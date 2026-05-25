//! Per-check submodule (see lib.rs for the dispatcher).

use hickory_resolver::TokioResolver;
use std::net::IpAddr;
use hickory_resolver::proto::rr::RData;

use super::{CheckResult, Status};

pub(super) async fn check_ptr(resolver: &TokioResolver, hostname: &str) -> CheckResult {
    // resolve hostname to IP, then reverse lookup
    let ip: Option<IpAddr> = match resolver.lookup_ip(hostname).await {
        Ok(ips) => ips.iter().next(),
        Err(e) => {
            return CheckResult {
                name: "Reverse DNS (PTR)".into(),
                status: Status::Fail,
                message: format!("could not resolve hostname {hostname}: {e}"),
                details: vec![],
            };
        }
    };
    let Some(ip) = ip else {
        return CheckResult {
            name: "Reverse DNS (PTR)".into(),
            status: Status::Fail,
            message: format!("no A/AAAA record for {hostname}"),
            details: vec![],
        };
    };

    match resolver.reverse_lookup(ip).await {
        Ok(names) => {
            let ptrs: Vec<String> = names
                .answers()
                .iter()
                .filter_map(|r| match &r.data {
                    RData::PTR(name) => Some(name.to_string()),
                    _ => None,
                })
                .collect();
            let matches = ptrs.iter().any(|n| n.trim_end_matches('.') == hostname);
            if matches {
                CheckResult {
                    name: "Reverse DNS (PTR)".into(),
                    status: Status::Pass,
                    message: format!("PTR for {ip} matches {hostname}"),
                    details: ptrs,
                }
            } else {
                CheckResult {
                    name: "Reverse DNS (PTR)".into(),
                    status: Status::Warn,
                    message: format!("PTR for {ip} does not match {hostname}"),
                    details: ptrs,
                }
            }
        }
        Err(e) => CheckResult {
            name: "Reverse DNS (PTR)".into(),
            status: Status::Warn,
            message: format!("reverse lookup for {ip} failed: {e}"),
            details: vec![],
        },
    }
}

