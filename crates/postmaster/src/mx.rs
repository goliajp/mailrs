//! Per-check submodule (see lib.rs for the dispatcher).

use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;

use super::{CheckResult, Status};

pub(super) async fn check_mx(resolver: &TokioResolver, domain: &str, hostname: &str) -> CheckResult {
    match resolver.mx_lookup(domain).await {
        Ok(records) => {
            let mxs: Vec<_> = records
                .answers()
                .iter()
                .filter_map(|r| match &r.data {
                    RData::MX(mx) => Some(mx),
                    _ => None,
                })
                .collect();
            let entries: Vec<String> = mxs
                .iter()
                .map(|mx| format!("{} (priority {})", mx.exchange, mx.preference))
                .collect();
            if entries.is_empty() {
                return CheckResult {
                    name: "MX Records".into(),
                    status: Status::Fail,
                    message: "no MX records found".into(),
                    details: vec![],
                };
            }
            // check if any MX points to our hostname
            let points_to_us = mxs.iter().any(|mx| {
                let exchange = mx.exchange.to_string();
                let exchange = exchange.trim_end_matches('.');
                exchange.eq_ignore_ascii_case(hostname)
            });
            if points_to_us {
                CheckResult {
                    name: "MX Records".into(),
                    status: Status::Pass,
                    message: format!("{} MX record(s) found, includes {hostname}", entries.len()),
                    details: entries,
                }
            } else {
                CheckResult {
                    name: "MX Records".into(),
                    status: Status::Warn,
                    message: format!(
                        "{} MX record(s) found, but none point to {hostname}",
                        entries.len()
                    ),
                    details: entries,
                }
            }
        }
        Err(e) => CheckResult {
            name: "MX Records".into(),
            status: Status::Fail,
            message: format!("MX lookup failed: {e}"),
            details: vec![],
        },
    }
}

