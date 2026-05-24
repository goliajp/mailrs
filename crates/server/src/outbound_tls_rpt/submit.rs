//! `submit_report` — daily TLSRPT submission flow: lookup
//! `_smtp._tls.<domain>`, parse RUA endpoints, dispatch via
//! mailto: (outbound queue) and https: (POST gzipped report).


use sqlx::PgPool;

use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;
use mailrs_tls_rpt::{
    Report, RuaEndpoint, SubmissionEmailOpts, TlsRptRecord,
    build_submission_email, gzip_report,
};


pub async fn submit_report(
    report: &Report,
    submitter_domain: &str,
    submitter_address: &str,
    resolver: &TokioResolver,
    outbound_pool: Option<&PgPool>,
    http_client: Option<&reqwest::Client>,
) -> (usize, usize) {
    let gzipped = match gzip_report(report) {
        Ok(g) => g,
        Err(e) => {
            tracing::error!(
                event = "tls_rpt_gzip_failed",
                error = %e,
                "TLSRPT submission aborted — gzip step failed"
            );
            return (0, report.policies.len());
        }
    };
    let date_rfc2822 = chrono::Utc::now()
        .format("%a, %d %b %Y %H:%M:%S +0000")
        .to_string();

    let mut ok = 0usize;
    let mut failed = 0usize;
    for policy in &report.policies {
        let receiving_domain = policy.policy.policy_domain.as_str();
        let Some(record) = lookup_tlsrpt_record(resolver, receiving_domain).await else {
            continue;
        };
        for endpoint in &record.rua {
            let succeeded = match endpoint {
                RuaEndpoint::Mailto(addr) => {
                    submit_via_mailto(
                        outbound_pool,
                        report,
                        submitter_address,
                        submitter_domain,
                        receiving_domain,
                        addr,
                        &gzipped,
                        &date_rfc2822,
                    )
                    .await
                }
                RuaEndpoint::Https(url) => {
                    submit_via_https(http_client, receiving_domain, url, &gzipped).await
                }
            };
            if succeeded {
                ok += 1;
            } else {
                failed += 1;
            }
        }
    }
    (ok, failed)
}

/// Resolve `_smtp._tls.<domain>` TXT, concatenate answers, parse
/// the RFC 8460 TLSRPT record. Returns `None` (and logs) on any
/// failure — the caller continues to the next policy.
async fn lookup_tlsrpt_record(
    resolver: &TokioResolver,
    receiving_domain: &str,
) -> Option<TlsRptRecord> {
    let q = format!("_smtp._tls.{receiving_domain}");
    let lookup = match resolver.txt_lookup(&q).await {
        Ok(r) => r,
        Err(e) => {
            tracing::info!(
                event = "tls_rpt_no_record",
                domain = receiving_domain,
                error = %e,
                "_smtp._tls TXT lookup failed — receiving domain doesn't publish TLSRPT"
            );
            return None;
        }
    };
    let txt: String = lookup
        .answers()
        .iter()
        .filter_map(|r| match &r.data {
            RData::TXT(t) => Some(t.to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");
    match TlsRptRecord::parse(&txt) {
        Ok(r) => Some(r),
        Err(e) => {
            tracing::warn!(
                event = "tls_rpt_record_parse_failed",
                domain = receiving_domain,
                error = %e,
                "TLSRPT record at {q} failed to parse — skipping submission"
            );
            None
        }
    }
}

/// Wrap the gzipped report in an RFC 8460 §3 MIME envelope and
/// enqueue it on the outbound queue addressed to `mailto:`
/// recipient. Returns true on enqueue success.
#[allow(clippy::too_many_arguments)]
async fn submit_via_mailto(
    outbound_pool: Option<&PgPool>,
    report: &Report,
    submitter_address: &str,
    submitter_domain: &str,
    receiving_domain: &str,
    addr: &str,
    gzipped: &[u8],
    date_rfc2822: &str,
) -> bool {
    let Some(pool) = outbound_pool else {
        tracing::warn!(
            event = "tls_rpt_mailto_no_queue",
            domain = receiving_domain,
            endpoint = %addr,
            "TLSRPT mailto: submission requires an outbound queue; skipping"
        );
        return false;
    };
    let boundary = format!(
        "tlsrpt-{}-{}",
        report.report_id.replace(['<', '>', '@'], "-"),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    );
    let opts = SubmissionEmailOpts {
        from_address: submitter_address.to_string(),
        to_address: addr.to_string(),
        receiving_domain: receiving_domain.to_string(),
        submitter_domain: submitter_domain.to_string(),
        report_id: report.report_id.clone(),
        date_rfc2822: date_rfc2822.to_string(),
        boundary,
        report_gzipped: gzipped.to_vec(),
    };
    let email = build_submission_email(&opts);
    let recipient_domain = addr.rsplit_once('@').map(|(_, d)| d).unwrap_or(addr);
    let now = chrono::Utc::now().timestamp();
    match mailrs_outbound_queue::queue::enqueue(
        pool,
        submitter_address,
        addr,
        recipient_domain,
        &email,
        Some(&report.report_id),
        now,
    )
    .await
    {
        Ok(queue_id) => {
            tracing::info!(
                event = "tls_rpt_mailto_enqueued",
                domain = receiving_domain,
                endpoint = %addr,
                queue_id = queue_id,
                "TLSRPT mailto report enqueued"
            );
            true
        }
        Err(e) => {
            tracing::warn!(
                event = "tls_rpt_mailto_enqueue_failed",
                domain = receiving_domain,
                endpoint = %addr,
                error = %e,
                "TLSRPT mailto enqueue failed"
            );
            false
        }
    }
}

/// POST the gzipped report body to the `https:` rua endpoint
/// with the RFC 8460 §3 Content-Type. Returns true on 2xx.
async fn submit_via_https(
    http_client: Option<&reqwest::Client>,
    receiving_domain: &str,
    url: &str,
    gzipped: &[u8],
) -> bool {
    let Some(client) = http_client else {
        tracing::warn!(
            event = "tls_rpt_https_no_client",
            domain = receiving_domain,
            endpoint = %url,
            "TLSRPT https: submission requires a reqwest client; skipping"
        );
        return false;
    };
    match client
        .post(url)
        .header("Content-Type", "application/tlsrpt+gzip")
        .body(gzipped.to_vec())
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            tracing::info!(
                event = "tls_rpt_https_submitted",
                domain = receiving_domain,
                endpoint = %url,
                status = %resp.status(),
                "TLSRPT https report submitted"
            );
            true
        }
        Ok(resp) => {
            tracing::warn!(
                event = "tls_rpt_https_non_2xx",
                domain = receiving_domain,
                endpoint = %url,
                status = %resp.status(),
                "TLSRPT https endpoint returned non-2xx"
            );
            false
        }
        Err(e) => {
            tracing::warn!(
                event = "tls_rpt_https_send_failed",
                domain = receiving_domain,
                endpoint = %url,
                error = %e,
                "TLSRPT https POST failed"
            );
            false
        }
    }
}
