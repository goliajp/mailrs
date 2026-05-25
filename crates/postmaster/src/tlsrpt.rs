//! Per-check submodule (see lib.rs for the dispatcher).

use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;

use super::{CheckResult, Status};

pub(super) async fn check_tlsrpt(resolver: &TokioResolver, domain: &str) -> CheckResult {
    let qname = format!("_smtp._tls.{domain}");
    match resolver.txt_lookup(&qname).await {
        Ok(records) => {
            let tls_records: Vec<String> = records
                .answers()
                .iter()
                .filter_map(|r| match &r.data {
                    RData::TXT(txt) => Some(txt.to_string()),
                    _ => None,
                })
                .filter(|txt| txt.contains("v=TLSRPTv1"))
                .collect();
            if tls_records.is_empty() {
                CheckResult {
                    name: "TLSRPT Record".into(),
                    status: Status::Warn,
                    message: "no TLSRPT record found".into(),
                    details: vec![],
                }
            } else {
                CheckResult {
                    name: "TLSRPT Record".into(),
                    status: Status::Pass,
                    message: "TLSRPT record found".into(),
                    details: tls_records,
                }
            }
        }
        Err(_) => CheckResult {
            name: "TLSRPT Record".into(),
            status: Status::Warn,
            message: "no TLSRPT record found".into(),
            details: vec![],
        },
    }
}

/// Quick syntactic validation: returns true iff the record contains
/// `v=TLSRPTv1` and at least one `rua=` clause. Tolerant of whitespace;
/// does not validate the URI inside `rua=`.
pub fn validate_tlsrpt_record(record: &str) -> bool {
    record.contains("v=TLSRPTv1") && record.contains("rua=")
}

/// Extract reporting URI(s) from a TLSRPT record.
///
/// TLSRPT format: `v=TLSRPTv1; rua=mailto:reports@example.com,https://...`.
/// Returns each comma-separated URI in `rua=` as a separate `String`, or
/// an empty `Vec` when the `rua=` field is missing.

pub fn extract_tlsrpt_rua(record: &str) -> Vec<String> {
    record
        .split(';')
        .map(|part| part.trim())
        .find(|part| part.starts_with("rua="))
        .map(|rua_part| {
            rua_part[4..]
                .split(',')
                .map(|uri| uri.trim().to_string())
                .collect()
        })
        .unwrap_or_default()
}
