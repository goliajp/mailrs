//! Per-check submodule (see lib.rs for the dispatcher).

use super::resolver::PostmasterResolver;
use super::{CheckResult, Status};

pub(super) async fn check_dmarc<R: PostmasterResolver + ?Sized>(
    resolver: &R,
    domain: &str,
) -> CheckResult {
    let qname = format!("_dmarc.{domain}");
    match resolver.txt_lookup(&qname).await {
        Ok(records) => {
            let dmarc_records: Vec<String> = records
                .into_iter()
                .filter(|txt| txt.starts_with("v=DMARC1"))
                .collect();
            if dmarc_records.is_empty() {
                return CheckResult {
                    name: "DMARC Record".into(),
                    status: Status::Fail,
                    message: "no DMARC record found".into(),
                    details: vec![],
                };
            }
            let record = &dmarc_records[0];
            let (status, policy_msg) = if record.contains("p=reject") {
                (Status::Pass, "policy: reject")
            } else if record.contains("p=quarantine") {
                (Status::Pass, "policy: quarantine")
            } else if record.contains("p=none") {
                (Status::Warn, "policy: none (monitoring only)")
            } else {
                (Status::Warn, "policy not recognized")
            };
            CheckResult {
                name: "DMARC Record".into(),
                status,
                message: format!("DMARC record found, {policy_msg}"),
                details: dmarc_records,
            }
        }
        Err(e) => CheckResult {
            name: "DMARC Record".into(),
            status: Status::Fail,
            message: format!("DMARC lookup failed: {e}"),
            details: vec![],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::MockResolver;

    #[tokio::test]
    async fn no_record_yields_fail() {
        let r = MockResolver::new();
        let res = check_dmarc(&r, "example.com").await;
        assert!(matches!(res.status, Status::Fail));
    }

    #[tokio::test]
    async fn p_reject_yields_pass() {
        let r = MockResolver::new().with_txt(
            "_dmarc.example.com",
            vec!["v=DMARC1; p=reject; rua=mailto:r@x".into()],
        );
        let res = check_dmarc(&r, "example.com").await;
        assert!(matches!(res.status, Status::Pass));
        assert!(res.message.contains("reject"));
    }

    #[tokio::test]
    async fn p_quarantine_yields_pass() {
        let r = MockResolver::new()
            .with_txt("_dmarc.example.com", vec!["v=DMARC1; p=quarantine".into()]);
        let res = check_dmarc(&r, "example.com").await;
        assert!(matches!(res.status, Status::Pass));
        assert!(res.message.contains("quarantine"));
    }

    #[tokio::test]
    async fn p_none_yields_warn() {
        let r = MockResolver::new().with_txt("_dmarc.example.com", vec!["v=DMARC1; p=none".into()]);
        let res = check_dmarc(&r, "example.com").await;
        assert!(matches!(res.status, Status::Warn));
    }

    #[tokio::test]
    async fn malformed_policy_yields_warn() {
        // No p= tag at all — falls through to "policy not recognized"
        let r = MockResolver::new().with_txt(
            "_dmarc.example.com",
            vec!["v=DMARC1; rua=mailto:r@x".into()],
        );
        let res = check_dmarc(&r, "example.com").await;
        assert!(matches!(res.status, Status::Warn));
        assert!(res.message.contains("not recognized"));
    }

    #[tokio::test]
    async fn non_dmarc_txt_filtered_out() {
        let r = MockResolver::new().with_txt("_dmarc.example.com", vec!["something else".into()]);
        let res = check_dmarc(&r, "example.com").await;
        assert!(matches!(res.status, Status::Fail));
    }
}
