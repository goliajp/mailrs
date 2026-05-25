//! Per-check submodule (see lib.rs for the dispatcher).

use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;

use super::{CheckResult, Status};

pub(super) async fn check_dkim(
    resolver: &TokioResolver,
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
                .answers()
                .iter()
                .filter_map(|r| match &r.data {
                    RData::TXT(txt) => Some(txt.to_string()),
                    _ => None,
                })
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
