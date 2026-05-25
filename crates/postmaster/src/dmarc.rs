//! Per-check submodule (see lib.rs for the dispatcher).

use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;

use super::{CheckResult, Status};

pub(super) async fn check_dmarc(resolver: &TokioResolver, domain: &str) -> CheckResult {
    let qname = format!("_dmarc.{domain}");
    match resolver.txt_lookup(&qname).await {
        Ok(records) => {
            let dmarc_records: Vec<String> = records
                .answers()
                .iter()
                .filter_map(|r| match &r.data {
                    RData::TXT(txt) => Some(txt.to_string()),
                    _ => None,
                })
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

