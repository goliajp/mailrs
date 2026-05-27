//! Per-check submodule (see lib.rs for the dispatcher).

use super::resolver::PostmasterResolver;
use super::{CheckResult, Status};

pub(super) async fn check_dkim<R: PostmasterResolver + ?Sized>(
    resolver: &R,
    domain: &str,
    selector: Option<&str>,
) -> CheckResult {
    let Some(sel) = selector else {
        return CheckResult {
            name: "DKIM Record".into(),
            status: Status::Skip,
            message: "no DKIM selector configured".into(),
            details: vec![],
        };
    };

    let qname = format!("{sel}._domainkey.{domain}");
    match resolver.txt_lookup(&qname).await {
        Ok(records) => {
            let dkim_records: Vec<String> = records
                .into_iter()
                .filter(|txt| txt.contains("v=DKIM1"))
                .collect();
            if dkim_records.is_empty() {
                CheckResult {
                    name: "DKIM Record".into(),
                    status: Status::Fail,
                    message: format!("no DKIM record at {qname}"),
                    details: vec![],
                }
            } else {
                CheckResult {
                    name: "DKIM Record".into(),
                    status: Status::Pass,
                    message: format!("DKIM record found at {qname}"),
                    details: dkim_records,
                }
            }
        }
        Err(e) => CheckResult {
            name: "DKIM Record".into(),
            status: Status::Fail,
            message: format!("DKIM lookup failed for {qname}: {e}"),
            details: vec![],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::MockResolver;

    #[tokio::test]
    async fn missing_selector_yields_skip() {
        let r = MockResolver::new();
        let res = check_dkim(&r, "example.com", None).await;
        assert!(matches!(res.status, Status::Skip));
        assert!(res.message.contains("no DKIM selector"));
    }

    #[tokio::test]
    async fn dkim_record_found_yields_pass() {
        let r = MockResolver::new().with_txt(
            "default._domainkey.example.com",
            vec!["v=DKIM1; k=rsa; p=MIGfMA0...".into()],
        );
        let res = check_dkim(&r, "example.com", Some("default")).await;
        assert!(matches!(res.status, Status::Pass));
        assert_eq!(res.details.len(), 1);
    }

    #[tokio::test]
    async fn dkim_lookup_nxdomain_yields_fail() {
        let r = MockResolver::new();
        let res = check_dkim(&r, "example.com", Some("missing")).await;
        assert!(matches!(res.status, Status::Fail));
    }

    #[tokio::test]
    async fn non_dkim_txt_filtered_out() {
        let r = MockResolver::new().with_txt(
            "default._domainkey.example.com",
            vec!["unrelated TXT".into()],
        );
        let res = check_dkim(&r, "example.com", Some("default")).await;
        assert!(matches!(res.status, Status::Fail));
    }
}
