//! Store inbound DMARC aggregate reports in the network kevy.
//!
//! The parsing itself lives in the `mailrs-dmarc` stone
//! (`mailrs_dmarc::ingest`); this module is the storage half — it owns
//! the key layout and the idempotency rule, nothing else.
//!
//! Layout, alongside the existing outbound-direction `mailrs:dmarc:*`
//! keys:
//!
//! ```text
//! mailrs:dmarcrpt:report:<sid>  hash  report metadata + rollup counts
//! mailrs:dmarcrpt:rows:<sid>    zset  member = row JSON, score = count
//! mailrs:dmarcrpt:index         zset  member = <sid>, score = window begin
//! ```
//!
//! `<sid>` is `<org_name>!<report_id>`, both sanitized. Report IDs are
//! meant to be globally unique but are only reliably unique *per
//! reporter*, so the org name is part of the key.
//!
//! Per-source aggregates are deliberately **not** stored. They are a
//! derivation (`common/data-architecture.md`) and the read path
//! recomputes them from rows — at a few reports per day there is
//! nothing to gain from a second copy that could drift.
//!
//! Everything is best-effort: a report that fails to store must never
//! affect delivery of the message carrying it.

use mailrs_dmarc::ingest::AggregateReport;

/// Key prefix for the per-report metadata hash.
const REPORT_PREFIX: &str = "mailrs:dmarcrpt:report:";
/// Key prefix for the per-report row zset.
const ROWS_PREFIX: &str = "mailrs:dmarcrpt:rows:";
/// Time-ordered index of every stored report.
pub const INDEX_KEY: &[u8] = b"mailrs:dmarcrpt:index";

/// What happened when a report was offered to the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreOutcome {
    /// The report was new; metadata, rows, and index were written.
    Stored,
    /// A report with this `<sid>` was already present; nothing written.
    Duplicate,
}

/// Default collector mailbox when `MAILRS_DMARC_REPORT_MAILBOX` is unset.
const DEFAULT_COLLECTOR: &str = "dmarc";

/// Parse and store any aggregate reports carried by a message being
/// delivered to `addr`.
///
/// Called from the spool drain before any delivery decision runs.
/// Delivery is never affected: the report still lands in the mailbox as
/// ordinary mail, and every failure here is logged and swallowed.
///
/// Messages to any other address short-circuit on the recipient check,
/// so ordinary mail never reaches the MIME walk.
pub fn maybe_ingest(addr: &str, body: &[u8]) {
    if !is_collector(addr) {
        return;
    }
    let Some(url) = crate::live_sync::network_kevy_url() else {
        tracing::debug!("no network kevy — DMARC report ingestion disabled");
        return;
    };
    for payload in mailrs_dmarc::ingest::extract_report_payloads(body) {
        let report = match mailrs_dmarc::ingest::parse_aggregate_report(&payload) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    event = "dmarc_report_unparseable",
                    recipient = %addr,
                    error = %e,
                    "attachment looked like a DMARC report but did not parse"
                );
                continue;
            }
        };
        match store_report(&url, &report) {
            Ok(StoreOutcome::Stored) => tracing::info!(
                event = "dmarc_report_ingested",
                org = %report.report_metadata.org_name,
                domain = %report.policy_published.domain,
                rows = report.records.len(),
                total = report.total_count(),
                passing = report.passing_count(),
                "stored DMARC aggregate report"
            ),
            Ok(StoreOutcome::Duplicate) => tracing::debug!(
                event = "dmarc_report_duplicate",
                org = %report.report_metadata.org_name,
                "report already stored; skipped"
            ),
            Err(e) => tracing::warn!(
                event = "dmarc_report_store_failed",
                error = %e,
                "could not store DMARC report"
            ),
        }
    }
}

/// Whether `addr` is the configured report collector, reading
/// `MAILRS_DMARC_REPORT_MAILBOX`.
fn is_collector(addr: &str) -> bool {
    let configured =
        std::env::var("MAILRS_DMARC_REPORT_MAILBOX").unwrap_or_else(|_| DEFAULT_COLLECTOR.into());
    matches_collector(&configured, addr)
}

/// Match `addr` against a collector setting.
///
/// The setting accepts either a bare local-part (`dmarc`, matching that
/// mailbox on every hosted domain) or a full address (`dmarc@golia.jp`,
/// matching only that one). Split from [`is_collector`] so the matching
/// rule is testable without mutating process environment.
fn matches_collector(configured: &str, addr: &str) -> bool {
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

/// Storage id for a report: `<org_name>!<report_id>`, sanitized.
pub fn storage_id(report: &AggregateReport) -> String {
    format!(
        "{}!{}",
        sanitize(&report.report_metadata.org_name),
        sanitize(&report.report_metadata.report_id)
    )
}

/// Restrict a key component to characters that cannot confuse the key
/// space. Report IDs and org names come off the wire, so they are
/// untrusted input to key construction.
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

/// Persist a parsed report. Idempotent by `<sid>`: a report already
/// present is left untouched and reported as [`StoreOutcome::Duplicate`]
/// rather than rewritten, so a receiver re-sending the same report
/// costs one read and no writes.
pub fn store_report(kevy_url: &str, report: &AggregateReport) -> std::io::Result<StoreOutcome> {
    let mut conn = kevy_client::Connection::open(kevy_url).map_err(std::io::Error::other)?;
    let sid = storage_id(report);
    let report_key = format!("{REPORT_PREFIX}{sid}");

    // Conditional write — see module docs. Receivers do re-send.
    if conn
        .hget(report_key.as_bytes(), b"report_id")
        .map_err(std::io::Error::other)?
        .is_some()
    {
        return Ok(StoreOutcome::Duplicate);
    }

    let meta = report_metadata_fields(report);
    let pairs: Vec<(&[u8], &[u8])> = meta
        .iter()
        .map(|(k, v)| (k.as_bytes(), v.as_bytes()))
        .collect();
    conn.hset(report_key.as_bytes(), &pairs)
        .map_err(std::io::Error::other)?;

    let rows_key = format!("{ROWS_PREFIX}{sid}");
    let row_blobs: Vec<String> = report.records.iter().map(row_json).collect();
    let members: Vec<(f64, &[u8])> = report
        .records
        .iter()
        .zip(row_blobs.iter())
        .map(|(rec, blob)| (rec.row.count as f64, blob.as_bytes()))
        .collect();
    if !members.is_empty() {
        conn.zadd(rows_key.as_bytes(), &members)
            .map_err(std::io::Error::other)?;
    }

    conn.zadd(
        INDEX_KEY,
        &[(
            report.report_metadata.date_range.begin as f64,
            sid.as_bytes(),
        )],
    )
    .map_err(std::io::Error::other)?;

    Ok(StoreOutcome::Stored)
}

/// Flatten report metadata into hash field/value pairs.
fn report_metadata_fields(report: &AggregateReport) -> Vec<(String, String)> {
    let m = &report.report_metadata;
    let p = &report.policy_published;
    let mut fields = vec![
        ("report_id".into(), m.report_id.clone()),
        ("org_name".into(), m.org_name.clone()),
        ("email".into(), m.email.clone()),
        ("begin".into(), m.date_range.begin.to_string()),
        ("end".into(), m.date_range.end.to_string()),
        ("policy_domain".into(), p.domain.clone()),
        ("p".into(), p.p.clone()),
        ("total".into(), report.total_count().to_string()),
        ("passing".into(), report.passing_count().to_string()),
        ("rows".into(), report.records.len().to_string()),
    ];
    if let Some(sp) = &p.sp {
        fields.push(("sp".into(), sp.clone()));
    }
    if let Some(pct) = p.pct {
        fields.push(("pct".into(), pct.to_string()));
    }
    if let Some(adkim) = &p.adkim {
        fields.push(("adkim".into(), adkim.clone()));
    }
    if let Some(aspf) = &p.aspf {
        fields.push(("aspf".into(), aspf.clone()));
    }
    fields
}

/// One row as the JSON blob stored in the rows zset.
fn row_json(rec: &mailrs_dmarc::ingest::ReportRecord) -> String {
    serde_json::json!({
        "source_ip": rec.row.source_ip,
        "count": rec.row.count,
        "disposition": rec.row.policy_evaluated.disposition,
        "dkim": rec.row.policy_evaluated.dkim,
        "spf": rec.row.policy_evaluated.spf,
        "header_from": rec.identifiers.header_from,
        "envelope_from": rec.identifiers.envelope_from,
        "passed": rec.passed(),
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPORT: &str = r#"<feedback>
  <report_metadata>
    <org_name>google.com</org_name>
    <email>noreply-dmarc-support@google.com</email>
    <report_id>1234567890</report_id>
    <date_range><begin>1784937600</begin><end>1785023999</end></date_range>
  </report_metadata>
  <policy_published>
    <domain>golia.jp</domain><adkim>r</adkim><aspf>r</aspf>
    <p>quarantine</p><sp>quarantine</sp><pct>100</pct>
  </policy_published>
  <record>
    <row><source_ip>203.0.113.10</source_ip><count>5</count>
      <policy_evaluated><disposition>none</disposition><dkim>pass</dkim><spf>pass</spf></policy_evaluated>
    </row>
    <identifiers><header_from>golia.jp</header_from></identifiers>
  </record>
</feedback>"#;

    fn parsed() -> AggregateReport {
        mailrs_dmarc::ingest::parse_aggregate_report(REPORT.as_bytes()).expect("fixture parses")
    }

    #[test]
    fn local_part_setting_matches_that_mailbox_on_any_domain() {
        assert!(matches_collector("dmarc", "dmarc@golia.jp"));
        assert!(matches_collector("dmarc", "dmarc@golia.ai"));
        assert!(matches_collector("dmarc", "DMARC@Golia.JP"));
        assert!(matches_collector("dmarc@", "dmarc@doracawl.com"));
    }

    #[test]
    fn full_address_setting_matches_only_that_address() {
        assert!(matches_collector("dmarc@golia.jp", "dmarc@golia.jp"));
        assert!(!matches_collector("dmarc@golia.jp", "dmarc@golia.ai"));
    }

    #[test]
    fn ordinary_recipients_never_match() {
        assert!(!matches_collector("dmarc", "lihao@golia.jp"));
        assert!(!matches_collector("dmarc", "postmaster@golia.jp"));
        // substring, not the local-part — must not match
        assert!(!matches_collector("dmarc", "dmarc-reports@golia.jp"));
        assert!(!matches_collector("dmarc", "notdmarc@golia.jp"));
    }

    #[test]
    fn a_bare_local_part_with_no_domain_is_not_a_recipient() {
        assert!(!matches_collector("dmarc", "dmarc"));
    }

    #[test]
    fn storage_id_combines_org_and_report_id() {
        assert_eq!(storage_id(&parsed()), "google.com!1234567890");
    }

    #[test]
    fn sanitize_neutralises_key_separators() {
        assert_eq!(sanitize("a:b c/d"), "a_b_c_d");
        assert_eq!(sanitize("mailrs:dmarcrpt:index"), "mailrs_dmarcrpt_index");
    }

    #[test]
    fn sanitize_keeps_ordinary_identifiers_intact() {
        assert_eq!(sanitize("google.com"), "google.com");
        assert_eq!(sanitize("report-id_1@x"), "report-id_1@x");
    }

    #[test]
    fn sanitize_falls_back_when_everything_is_stripped() {
        assert_eq!(sanitize(""), "unknown");
        assert_eq!(sanitize(":::"), "___");
    }

    #[test]
    fn sanitize_bounds_length() {
        assert_eq!(sanitize(&"a".repeat(500)).len(), 120);
    }

    #[test]
    fn metadata_fields_carry_rollup_counts() {
        let fields = report_metadata_fields(&parsed());
        let get = |k: &str| {
            fields
                .iter()
                .find(|(f, _)| f == k)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };

        assert_eq!(get("org_name"), "google.com");
        assert_eq!(get("policy_domain"), "golia.jp");
        assert_eq!(get("total"), "5");
        assert_eq!(get("passing"), "5");
        assert_eq!(get("rows"), "1");
        assert_eq!(get("pct"), "100");
    }

    #[test]
    fn row_json_flattens_the_evaluated_verdict() {
        let report = parsed();
        let v: serde_json::Value =
            serde_json::from_str(&row_json(&report.records[0])).expect("valid json");

        assert_eq!(v["source_ip"], "203.0.113.10");
        assert_eq!(v["count"], 5);
        assert_eq!(v["disposition"], "none");
        assert_eq!(v["passed"], true);
        assert_eq!(v["envelope_from"], serde_json::Value::Null);
    }
}
