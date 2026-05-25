//! Per-check submodule (see lib.rs for the dispatcher).

use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;

use super::{CheckResult, Status};

pub(super) async fn check_bimi(resolver: &TokioResolver, domain: &str) -> CheckResult {
    let qname = format!("default._bimi.{domain}");
    match resolver.txt_lookup(&qname).await {
        Ok(records) => {
            let bimi_records: Vec<String> = records
                .answers()
                .iter()
                .filter_map(|r| match &r.data {
                    RData::TXT(txt) => Some(txt.to_string()),
                    _ => None,
                })
                .filter(|txt| txt.contains("v=BIMI1"))
                .collect();
            if bimi_records.is_empty() {
                CheckResult {
                    name: "BIMI Record".into(),
                    status: Status::Skip,
                    message: "no BIMI record found".into(),
                    details: vec![],
                }
            } else {
                let logo_url = extract_bimi_logo_url(&bimi_records[0]);
                let (status, message) = if logo_url.is_some() {
                    (Status::Pass, "BIMI record found with logo URL".to_string())
                } else {
                    (
                        Status::Warn,
                        "BIMI record found but no logo URL (l= tag missing)".to_string(),
                    )
                };
                CheckResult {
                    name: "BIMI Record".into(),
                    status,
                    message,
                    details: bimi_records,
                }
            }
        }
        Err(_) => CheckResult {
            name: "BIMI Record".into(),
            status: Status::Skip,
            message: "no BIMI record found".into(),
            details: vec![],
        },
    }
}

/// extract the logo URL from a BIMI record (l=https://...)
pub fn extract_bimi_logo_url(record: &str) -> Option<String> {
    record
        .split(';')
        .map(|part| part.trim())
        .find(|part| part.starts_with("l="))
        .and_then(|l_part| {
            let url = l_part[2..].trim();
            if url.is_empty() {
                None
            } else {
                Some(url.to_string())
            }
        })
}

/// look up BIMI record for a domain and return the logo URL if found
pub async fn lookup_bimi_logo(resolver: &TokioResolver, domain: &str) -> Option<String> {
    let qname = format!("default._bimi.{domain}");
    let records = resolver.txt_lookup(&qname).await.ok()?;
    records
        .answers()
        .iter()
        .filter_map(|r| match &r.data {
            RData::TXT(txt) => Some(txt.to_string()),
            _ => None,
        })
        .find(|txt| txt.contains("v=BIMI1"))
        .and_then(|rec| extract_bimi_logo_url(&rec))
}
