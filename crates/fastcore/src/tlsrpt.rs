//! TLS-RPT (RFC 8460) event recording + daily aggregate submission (G8.3).
//!
//! The sender records one compact event per delivery attempt into the
//! network kevy, bucketed by (receiving domain, UTC day):
//!
//!   RPUSH mailrs:tlsrpt:<domain>:<yyyymmdd> <json event>
//!
//! Once a day fastcore's submit task walks yesterday's buckets, and for
//! any domain that published a `_smtp._tls.<domain>` TXT policy with a
//! `rua=` endpoint, builds the RFC 8460 report from the stored events,
//! gzips it, and submits it (mailto → outbound queue as a null-envelope
//! message; https → POST). The bucket is deleted after a successful
//! submit so a report is sent at most once.
//!
//! Everything is fail-open + best-effort — TLS-RPT is diagnostic, it
//! must never affect delivery.

use std::sync::Arc;

use mailrs_tls_rpt::{
    FailureEvent, FailureType, PolicyType, ReportBuilder, RuaEndpoint, SubmissionEmailOpts,
    SuccessEvent, TlsRptRecord, build_submission_email, gzip_report,
};
use serde::{Deserialize, Serialize};

use crate::FastcoreState;

/// Compact wire form of a delivery TLS observation. Stored as JSON in
/// the per-domain daily kevy list; reconstructed into the stone's
/// `SuccessEvent` / `FailureEvent` at aggregation time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsEvent {
    /// Receiving domain.
    pub domain: String,
    /// MX host attempted.
    pub mx: String,
    /// `true` = TLS negotiated & delivered; `false` = failure.
    pub success: bool,
    /// RFC 8460 failure-type string (only when `!success`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_type: Option<String>,
    /// Free-text detail (only when `!success`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

fn day_key(domain: &str, day: &str) -> String {
    format!("mailrs:tlsrpt:{}:{day}", domain.to_ascii_lowercase())
}

fn utc_day(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|d| d.format("%Y%m%d").to_string())
        .unwrap_or_else(|| "00000000".into())
}

/// Record a TLS observation for later aggregation. Best-effort.
pub fn record(kevy_url: &str, ev: &TlsEvent) {
    let Ok(mut conn) = kevy_client::Connection::open(kevy_url) else {
        return;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let key = day_key(&ev.domain, &utc_day(now));
    if let Ok(json) = serde_json::to_vec(ev) {
        let _ = conn.rpush(key.as_bytes(), &[json.as_slice()]);
        // keep buckets from leaking if a domain never gets a report sent
        let _ = conn.expire(key.as_bytes(), std::time::Duration::from_secs(7 * 86_400));
    }
}

/// Spawn the daily submission task. Runs one sweep at boot (covering
/// yesterday) then every 24 h.
pub fn spawn_submit(state: Arc<FastcoreState>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let Some(url) = crate::live_sync::network_kevy_url() else {
            tracing::info!("no network kevy — TLS-RPT submit disabled");
            return;
        };
        loop {
            if let Err(e) = submit_yesterday(&state, &url).await {
                tracing::warn!(error = %e, "tls-rpt submit sweep failed");
            }
            tokio::time::sleep(std::time::Duration::from_secs(24 * 3600)).await;
        }
    })
}

async fn submit_yesterday(
    state: &Arc<FastcoreState>,
    url: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as i64;
    let yesterday = utc_day(now - 86_400);
    let submitter = std::env::var("MAILRS_HELO_HOSTNAME").unwrap_or_else(|_| "mailrs".into());

    // discover which domains have buckets for yesterday
    let pattern = format!("mailrs:tlsrpt:*:{yesterday}");
    let keys: Vec<String> = {
        let mut conn = kevy_client::Connection::open(url)?;
        conn.keys(pattern.as_bytes())?
            .into_iter()
            .filter_map(|k| String::from_utf8(k).ok())
            .collect()
    };
    for key in keys {
        // key = mailrs:tlsrpt:<domain>:<day>
        let parts: Vec<&str> = key.split(':').collect();
        let Some(domain) = parts.get(2).map(|s| s.to_string()) else {
            continue;
        };
        if let Err(e) = submit_domain(state, url, &domain, &yesterday, &submitter).await {
            tracing::warn!(%domain, error = %e, "tls-rpt: domain submit failed");
        }
    }
    Ok(())
}

async fn submit_domain(
    _state: &Arc<FastcoreState>,
    url: &str,
    domain: &str,
    day: &str,
    submitter: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. discover the rua endpoint (skip domains with no TLS-RPT policy)
    let Some(rua) = discover_rua(domain).await else {
        // no policy → drop the bucket, nobody to report to
        let mut conn = kevy_client::Connection::open(url)?;
        let _ = conn.del(&[day_key(domain, day).as_bytes()]);
        return Ok(());
    };

    // 2. load + aggregate the day's events
    let events: Vec<TlsEvent> = {
        let mut conn = kevy_client::Connection::open(url)?;
        conn.lrange(day_key(domain, day).as_bytes(), 0, -1)?
            .into_iter()
            .filter_map(|b| serde_json::from_slice(&b).ok())
            .collect()
    };
    if events.is_empty() {
        return Ok(());
    }
    let report_id = format!("{day}.{domain}@{submitter}");
    let mut b = ReportBuilder::new()
        .organization_name(submitter.to_string())
        .contact_info(format!("postmaster@{submitter}"))
        .report_id(report_id.clone())
        .date_range(format!("{day}T00:00:00Z"), format!("{day}T23:59:59Z"))
        .policy_string(domain, PolicyType::Sts, Vec::<String>::new());
    for ev in &events {
        if ev.success {
            b.record_success(SuccessEvent {
                policy_domain: domain.to_string(),
                policy_type: PolicyType::Sts,
                mx_host: ev.mx.clone(),
            });
        } else {
            b.record_failure(FailureEvent {
                policy_domain: domain.to_string(),
                policy_type: PolicyType::Sts,
                mx_host: Some(ev.mx.clone()),
                result_type: parse_failure_type(ev.failure_type.as_deref()),
                sending_mta_ip: None,
                receiving_ip: None,
                receiving_mx_helo: None,
                additional_information: ev.detail.clone(),
                failure_reason_code: None,
            });
        }
    }
    let report = b.build()?;
    let gz = gzip_report(&report)?;

    match rua {
        RuaEndpoint::Https(endpoint) => {
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(20))
                .build()?
                .post(&endpoint)
                .header("content-type", "application/tlsrpt+gzip")
                .body(gz)
                .send()
                .await?
                .error_for_status()?;
            tracing::info!(%domain, %endpoint, events = events.len(), "tls-rpt submitted (https)");
        }
        RuaEndpoint::Mailto(to) => {
            let opts = SubmissionEmailOpts {
                from_address: format!("tlsrpt@{submitter}"),
                to_address: to.clone(),
                receiving_domain: domain.to_string(),
                submitter_domain: submitter.to_string(),
                report_id,
                date_rfc2822: chrono::DateTime::from_timestamp(0, 0)
                    .map(|_| chrono::Utc::now().to_rfc2822())
                    .unwrap_or_default(),
                boundary: format!("=_tlsrpt_{day}"),
                report_gzipped: gz,
            };
            let email = build_submission_email(&opts);
            enqueue_report_email(url, &to, &email)?;
            tracing::info!(%domain, %to, events = events.len(), "tls-rpt queued (mailto)");
        }
    }
    // 3. drop the bucket — reported once
    let mut conn = kevy_client::Connection::open(url)?;
    let _ = conn.del(&[day_key(domain, day).as_bytes()]);
    Ok(())
}

/// Look up `_smtp._tls.<domain>` and return the first usable rua.
async fn discover_rua(domain: &str) -> Option<RuaEndpoint> {
    let resolver = mailrs_smtp_client::TokioResolver::builder_tokio()
        .ok()?
        .build()
        .ok()?;
    let name = format!("_smtp._tls.{domain}");
    let lookup = resolver.txt_lookup(name).await.ok()?;
    for rec in lookup.answers() {
        let txt = rec.to_string();
        if let Ok(parsed) = TlsRptRecord::parse(&txt) {
            return parsed.rua.into_iter().next();
        }
    }
    None
}

fn enqueue_report_email(url: &str, to: &str, email: &[u8]) -> std::io::Result<()> {
    use base64::Engine as _;
    let mut conn = kevy_client::Connection::open(url).map_err(std::io::Error::other)?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let id = format!("{now_ms}-tlsrpt");
    let env = serde_json::json!({
        "sender": "<>",
        "recipient": to,
        "message_data_b64": base64::engine::general_purpose::STANDARD.encode(email),
        "attempts": 0,
        "next_attempt": 0,
        "id": &id,
        "envelope_from": "<>",
    });
    let key = format!("mailrs:outbound:{id}");
    conn.hset(
        key.as_bytes(),
        &[(b"blob".as_slice(), env.to_string().as_bytes())],
    )
    .map_err(std::io::Error::other)?;
    conn.lpush(b"mailrs:outbound:pending", &[id.as_bytes()])
        .map_err(std::io::Error::other)?;
    Ok(())
}

fn parse_failure_type(s: Option<&str>) -> FailureType {
    match s.unwrap_or("") {
        "starttls-not-supported" => FailureType::StarttlsNotSupported,
        "certificate-host-mismatch" => FailureType::CertificateHostMismatch,
        "certificate-expired" => FailureType::CertificateExpired,
        "certificate-not-trusted" => FailureType::CertificateNotTrusted,
        "sts-policy-fetch-error" => FailureType::StsPolicyFetchError,
        "sts-webpki-invalid" => FailureType::StsWebpkiInvalid,
        "mx-mismatch" => FailureType::MxMismatch,
        "dane-required" => FailureType::DaneRequired,
        _ => FailureType::ValidationFailure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_roundtrips_json() {
        let ev = TlsEvent {
            domain: "example.com".into(),
            mx: "mx1.example.com".into(),
            success: false,
            failure_type: Some("mx-mismatch".into()),
            detail: Some("not in policy".into()),
        };
        let j = serde_json::to_vec(&ev).unwrap();
        let back: TlsEvent = serde_json::from_slice(&j).unwrap();
        assert_eq!(back.domain, "example.com");
        assert!(!back.success);
        assert_eq!(back.failure_type.as_deref(), Some("mx-mismatch"));
    }

    #[test]
    fn success_event_omits_failure_fields() {
        let ev = TlsEvent {
            domain: "x.y".into(),
            mx: "mx.x.y".into(),
            success: true,
            failure_type: None,
            detail: None,
        };
        let j = String::from_utf8(serde_json::to_vec(&ev).unwrap()).unwrap();
        assert!(!j.contains("failure_type"));
        assert!(!j.contains("detail"));
    }

    #[test]
    fn failure_type_mapping() {
        assert_eq!(
            parse_failure_type(Some("mx-mismatch")),
            FailureType::MxMismatch
        );
        assert_eq!(
            parse_failure_type(Some("garbage")),
            FailureType::ValidationFailure
        );
    }
}
