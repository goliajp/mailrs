//! Per-check submodule (see lib.rs for the dispatcher).

use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::{RData, RecordType};

use super::{CheckResult, Status};

pub(super) async fn check_dane(resolver: &TokioResolver, domain: &str) -> CheckResult {
    // look up MX first, then check TLSA for port 25 on first MX
    let mx_host = match resolver.mx_lookup(domain).await {
        Ok(records) => {
            let mut entries: Vec<_> = records
                .answers()
                .iter()
                .filter_map(|r| match &r.data {
                    RData::MX(mx) => Some(mx),
                    _ => None,
                })
                .collect();
            entries.sort_by_key(|mx| mx.preference);
            entries
                .first()
                .map(|mx| mx.exchange.to_string().trim_end_matches('.').to_string())
        }
        Err(_) => None,
    };
    let Some(mx_host) = mx_host else {
        return CheckResult {
            name: "DANE/TLSA".into(),
            status: Status::Skip,
            message: "no MX records, skipping DANE check".into(),
            details: vec![],
        };
    };

    let qname = format!("_25._tcp.{mx_host}");
    match resolver.lookup(&qname, RecordType::TLSA).await {
        Ok(records) => {
            let entries: Vec<String> = records
                .answers()
                .iter()
                .map(|r| format!("{}", r.data))
                .collect();
            if entries.is_empty() {
                CheckResult {
                    name: "DANE/TLSA".into(),
                    status: Status::Skip,
                    message: format!("no TLSA records at {qname}"),
                    details: vec![],
                }
            } else {
                CheckResult {
                    name: "DANE/TLSA".into(),
                    status: Status::Pass,
                    message: format!("TLSA record(s) found at {qname}"),
                    details: entries,
                }
            }
        }
        Err(_) => CheckResult {
            name: "DANE/TLSA".into(),
            status: Status::Skip,
            message: format!("no TLSA records at {qname} (DANE not configured)"),
            details: vec![],
        },
    }
}
