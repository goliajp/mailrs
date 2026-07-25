//! Store inbound TLS-RPT reports (RFC 8460).
//!
//! The mirror image of `dmarc_ingest`. Other MTAs mail us a daily JSON
//! summary of how TLS went when they delivered to us: how many sessions
//! succeeded, and — the part worth having — which ones failed and why.
//!
//! That failure list is the entire reason to publish a TLS-RPT record.
//! It is also the data that would have to be empty before `enforce`
//! mode could be considered, since enforce turns each of those failures
//! into a message that does not arrive.
//!
//! Layout, parallel to `mailrs:dmarcrpt:*`:
//!
//! ```text
//! mailrs:tlsrpt:report:<sid>    hash  metadata + success/failure totals
//! mailrs:tlsrpt:failures:<sid>  zset  member = failure JSON, score = count
//! mailrs:tlsrpt:index           zset  member = <sid>, score = window start
//! ```
//!
//! `<sid>` is `<organization-name>!<report-id>`, sanitized — same rule
//! as the DMARC side, and for the same reason: report ids are unique
//! per reporter, not globally.
//!
//! Best-effort throughout: a report that fails to store must never
//! affect delivery of the message carrying it.

use mailrs_tls_rpt::Report;

/// Key prefix for per-report metadata.
const REPORT_PREFIX: &str = "mailrs:tlsrpt:report:";
/// Key prefix for the per-report failure zset.
const FAILURES_PREFIX: &str = "mailrs:tlsrpt:failures:";
/// Time-ordered index of stored reports.
pub const INDEX_KEY: &[u8] = b"mailrs:tlsrpt:index";

/// Default collector mailbox when `MAILRS_TLSRPT_MAILBOX` is unset.
const DEFAULT_COLLECTOR: &str = "tlsrpt";

/// Parse and store any TLS-RPT reports carried by a message.
///
/// Called from the spool drain alongside the DMARC hook. Non-collector
/// recipients short-circuit before any MIME work.
pub fn maybe_ingest(addr: &str, body: &[u8]) {
    if !is_collector(addr) {
        return;
    }
    let Some(url) = crate::live_sync::network_kevy_url() else {
        tracing::debug!("no network kevy — TLS-RPT ingestion disabled");
        return;
    };
    // Attachment extraction is shared with the DMARC path: identifying
    // and decompressing a report attachment is the same problem for
    // both, down to the gzip/zip/plain split. If a third report format
    // ever needs it, it should move to its own stone.
    for payload in mailrs_dmarc::ingest::extract_report_payloads(body) {
        let report: Report = match serde_json::from_slice(&payload) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    event = "tlsrpt_report_unparseable",
                    recipient = %addr,
                    error = %e,
                    "attachment looked like a TLS-RPT report but did not parse"
                );
                continue;
            }
        };
        match store_report(&url, &report) {
            Ok(true) => {
                let (ok, failed) = totals(&report);
                tracing::info!(
                    event = "tlsrpt_report_ingested",
                    org = %report.organization_name,
                    successful = ok,
                    failed,
                    "stored TLS-RPT report"
                );
                if failed > 0 {
                    // The whole point of publishing the record. Log it
                    // loudly enough to notice without opening the UI.
                    tracing::warn!(
                        event = "tlsrpt_failures_reported",
                        org = %report.organization_name,
                        failed,
                        "a sending MTA reported TLS failures delivering to us"
                    );
                }
            }
            Ok(false) => tracing::debug!(
                event = "tlsrpt_report_duplicate",
                org = %report.organization_name,
                "report already stored; skipped"
            ),
            Err(e) => tracing::warn!(
                event = "tlsrpt_report_store_failed",
                error = %e,
                "could not store TLS-RPT report"
            ),
        }
    }
}

/// Whether `addr` is the configured TLS-RPT collector.
fn is_collector(addr: &str) -> bool {
    let configured =
        std::env::var("MAILRS_TLSRPT_MAILBOX").unwrap_or_else(|_| DEFAULT_COLLECTOR.into());
    let configured = configured.trim();
    match configured.split_once('@') {
        Some((_, domain)) if !domain.is_empty() => addr.eq_ignore_ascii_case(configured),
        _ => {
            let want = configured.trim_end_matches('@');
            addr.split_once('@')
                .map(|(local, _)| local.eq_ignore_ascii_case(want))
                .unwrap_or(false)
        }
    }
}

/// Storage id: `<organization-name>!<report-id>`, sanitized.
pub fn storage_id(report: &Report) -> String {
    format!(
        "{}!{}",
        sanitize(&report.organization_name),
        sanitize(&report.report_id)
    )
}

/// Restrict a key component to characters that cannot confuse the key
/// space — both fields come off the wire.
fn sanitize(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' | '@' => c,
            _ => '_',
        })
        .take(120)
        .collect();
    if cleaned.is_empty() {
        "unknown".into()
    } else {
        cleaned
    }
}

/// Successful and failed session counts, summed across policies.
pub fn totals(report: &Report) -> (u64, u64) {
    let mut ok = 0u64;
    let mut failed = 0u64;
    for p in &report.policies {
        ok += p.summary.total_successful_session_count;
        failed += p.summary.total_failure_session_count;
    }
    (ok, failed)
}

/// Persist a parsed report. Returns whether it was new.
fn store_report(kevy_url: &str, report: &Report) -> std::io::Result<bool> {
    let mut conn = kevy_client::Connection::open(kevy_url).map_err(std::io::Error::other)?;
    let sid = storage_id(report);
    let report_key = format!("{REPORT_PREFIX}{sid}");

    // Conditional write — reporters do re-send.
    if conn
        .hget(report_key.as_bytes(), b"report_id")
        .map_err(std::io::Error::other)?
        .is_some()
    {
        return Ok(false);
    }

    let (ok, failed) = totals(report);
    let begin = report.date_range.start_datetime.clone();
    let end = report.date_range.end_datetime.clone();
    let policy_domains: Vec<&str> = report
        .policies
        .iter()
        .map(|p| p.policy.policy_domain.as_str())
        .collect();
    let ok_s = ok.to_string();
    let failed_s = failed.to_string();
    let domains_s = policy_domains.join(",");
    conn.hset(
        report_key.as_bytes(),
        &[
            (b"report_id".as_slice(), report.report_id.as_bytes()),
            (b"org_name".as_slice(), report.organization_name.as_bytes()),
            (b"contact".as_slice(), report.contact_info.as_bytes()),
            (b"begin".as_slice(), begin.as_bytes()),
            (b"end".as_slice(), end.as_bytes()),
            (b"policy_domains".as_slice(), domains_s.as_bytes()),
            (b"successful".as_slice(), ok_s.as_bytes()),
            (b"failed".as_slice(), failed_s.as_bytes()),
        ],
    )
    .map_err(std::io::Error::other)?;

    // Failure detail, highest-count first when read back.
    let blobs: Vec<(f64, String)> = report
        .policies
        .iter()
        .flat_map(|p| {
            p.failure_details.iter().map(move |f| {
                (
                    f.failed_session_count as f64,
                    serde_json::json!({
                        "policy_domain": p.policy.policy_domain,
                        "result_type": format!("{:?}", f.result_type),
                        "count": f.failed_session_count,
                        "sending_mta_ip": f.sending_mta_ip,
                        "receiving_mx_hostname": f.receiving_mx_hostname,
                        "failure_reason_code": f.failure_reason_code,
                    })
                    .to_string(),
                )
            })
        })
        .collect();
    if !blobs.is_empty() {
        let members: Vec<(f64, &[u8])> = blobs.iter().map(|(c, b)| (*c, b.as_bytes())).collect();
        conn.zadd(format!("{FAILURES_PREFIX}{sid}").as_bytes(), &members)
            .map_err(std::io::Error::other)?;
    }

    // Index score is the window start as a unix timestamp. RFC 8460
    // gives RFC 3339 strings, so parse; an unparseable value sorts to
    // the front rather than dropping the report.
    let score = chrono::DateTime::parse_from_rfc3339(&begin)
        .map(|d| d.timestamp() as f64)
        .unwrap_or(0.0);
    conn.zadd(INDEX_KEY, &[(score, sid.as_bytes())])
        .map_err(std::io::Error::other)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPORT: &str = r#"{
      "organization-name": "Google Inc.",
      "date-range": {
        "start-datetime": "2026-07-25T00:00:00Z",
        "end-datetime": "2026-07-25T23:59:59Z"
      },
      "contact-info": "smtp-tls-reporting@google.com",
      "report-id": "2026-07-25T00:00:00Z_golia.jp",
      "policies": [{
        "policy": {
          "policy-type": "sts",
          "policy-string": ["version: STSv1", "mode: testing", "mx: mail.golia.ai"],
          "policy-domain": "golia.jp"
        },
        "summary": {
          "total-successful-session-count": 42,
          "total-failure-session-count": 3
        },
        "failure-details": [{
          "result-type": "certificate-expired",
          "sending-mta-ip": "209.85.220.41",
          "receiving-mx-hostname": "mail.golia.ai",
          "failed-session-count": 3
        }]
      }]
    }"#;

    fn parsed() -> Report {
        serde_json::from_str(REPORT).expect("fixture parses")
    }

    #[test]
    fn parses_a_real_report_shape() {
        let r = parsed();
        assert_eq!(r.organization_name, "Google Inc.");
        assert_eq!(r.policies.len(), 1);
        assert_eq!(r.policies[0].policy.policy_domain, "golia.jp");
    }

    #[test]
    fn totals_sum_across_policies() {
        let (ok, failed) = totals(&parsed());
        assert_eq!(ok, 42);
        assert_eq!(failed, 3);
    }

    #[test]
    fn storage_id_combines_org_and_report_id() {
        // Spaces and colons in both fields must not reach the key space.
        assert_eq!(
            storage_id(&parsed()),
            "Google_Inc.!2026-07-25T00_00_00Z_golia.jp"
        );
    }

    #[test]
    fn collector_matches_the_local_part_on_any_domain() {
        assert!(is_collector("tlsrpt@golia.jp"));
        assert!(is_collector("TLSRPT@golia.ai"));
        assert!(!is_collector("dmarc@golia.jp"));
        assert!(!is_collector("lihao@golia.jp"));
    }

    #[test]
    fn a_report_with_no_failures_still_parses() {
        let json = REPORT
            .replace(
                r#""total-failure-session-count": 3"#,
                r#""total-failure-session-count": 0"#,
            )
            .replace(
                r#""failure-details": [{
          "result-type": "certificate-expired",
          "sending-mta-ip": "209.85.220.41",
          "receiving-mx-hostname": "mail.golia.ai",
          "failed-session-count": 3
        }]"#,
                r#""failure-details": []"#,
            );
        let r: Report = serde_json::from_str(&json).expect("parses");
        assert_eq!(totals(&r), (42, 0));
    }
}
