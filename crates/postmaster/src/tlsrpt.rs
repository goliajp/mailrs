//! Per-check submodule (see lib.rs for the dispatcher).

use super::resolver::PostmasterResolver;
use super::{CheckResult, Status};

pub(super) async fn check_tlsrpt<R: PostmasterResolver + ?Sized>(
    resolver: &R,
    domain: &str,
) -> CheckResult {
    let qname = format!("_smtp._tls.{domain}");
    match resolver.txt_lookup(&qname).await {
        Ok(records) => {
            let tls_records: Vec<String> = records
                .into_iter()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::MockResolver;

    #[tokio::test]
    async fn no_record_yields_warn() {
        let r = MockResolver::new();
        let res = check_tlsrpt(&r, "example.com").await;
        assert!(matches!(res.status, Status::Warn));
    }

    #[tokio::test]
    async fn valid_record_yields_pass() {
        let r = MockResolver::new().with_txt(
            "_smtp._tls.example.com",
            vec!["v=TLSRPTv1; rua=mailto:tls-reports@example.com".into()],
        );
        let res = check_tlsrpt(&r, "example.com").await;
        assert!(matches!(res.status, Status::Pass));
    }

    #[tokio::test]
    async fn unrelated_txt_filtered_out() {
        let r = MockResolver::new()
            .with_txt("_smtp._tls.example.com", vec!["v=DMARC1; p=reject".into()]);
        let res = check_tlsrpt(&r, "example.com").await;
        assert!(matches!(res.status, Status::Warn));
    }

    #[test]
    fn validate_accepts_v1_with_rua() {
        assert!(validate_tlsrpt_record("v=TLSRPTv1; rua=mailto:r@x"));
    }

    #[test]
    fn validate_rejects_without_rua() {
        assert!(!validate_tlsrpt_record("v=TLSRPTv1"));
    }

    #[test]
    fn validate_rejects_without_v1() {
        assert!(!validate_tlsrpt_record("rua=mailto:r@x"));
    }

    #[test]
    fn extract_rua_single() {
        let uris = extract_tlsrpt_rua("v=TLSRPTv1; rua=mailto:r@x");
        assert_eq!(uris, vec!["mailto:r@x"]);
    }

    #[test]
    fn extract_rua_multiple() {
        let uris = extract_tlsrpt_rua("v=TLSRPTv1; rua=mailto:r@x, https://x/y");
        assert_eq!(uris, vec!["mailto:r@x", "https://x/y"]);
    }

    #[test]
    fn extract_rua_missing() {
        let uris = extract_tlsrpt_rua("v=TLSRPTv1");
        assert!(uris.is_empty());
    }
}
