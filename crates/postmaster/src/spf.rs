//! Per-check submodule (see lib.rs for the dispatcher).

use hickory_resolver::TokioResolver;
use std::net::IpAddr;
use hickory_resolver::proto::rr::RData;

use super::{CheckResult, Status};

pub(super) async fn check_spf(resolver: &TokioResolver, domain: &str, hostname: &str) -> CheckResult {
    // resolve our hostname to IPs for SPF inclusion check
    let our_ips: Vec<IpAddr> = resolver
        .lookup_ip(hostname)
        .await
        .map(|ips| ips.iter().collect())
        .unwrap_or_default();

    match resolver.txt_lookup(domain).await {
        Ok(records) => {
            let spf_records: Vec<String> = records
                .answers()
                .iter()
                .filter_map(|r| match &r.data {
                    RData::TXT(txt) => Some(txt.to_string()),
                    _ => None,
                })
                .filter(|txt| txt.starts_with("v=spf1"))
                .collect();
            match spf_records.len() {
                0 => CheckResult {
                    name: "SPF Record".into(),
                    status: Status::Fail,
                    message: "no SPF record found".into(),
                    details: vec![],
                },
                1 => {
                    let record = &spf_records[0];
                    // check if our hostname or IP is mentioned in the SPF record
                    let includes_us = record.contains(hostname)
                        || our_ips.iter().any(|ip| record.contains(&ip.to_string()));

                    let policy_note = if record.contains("-all") {
                        "strict (-all)"
                    } else if record.contains("~all") {
                        "soft fail (~all)"
                    } else if record.contains("?all") {
                        "neutral (?all)"
                    } else if record.contains("+all") {
                        "pass all (+all, dangerous)"
                    } else {
                        "unknown policy"
                    };

                    let (status, message) = if includes_us {
                        (
                            Status::Pass,
                            format!("SPF record found, {policy_note}, includes {hostname}"),
                        )
                    } else {
                        (
                            Status::Warn,
                            format!(
                                "SPF record found, {policy_note}, but does not include {hostname}"
                            ),
                        )
                    };

                    CheckResult {
                        name: "SPF Record".into(),
                        status,
                        message,
                        details: spf_records,
                    }
                }
                _ => CheckResult {
                    name: "SPF Record".into(),
                    status: Status::Warn,
                    message: "multiple SPF records found (should have exactly one)".into(),
                    details: spf_records,
                },
            }
        }
        Err(e) => CheckResult {
            name: "SPF Record".into(),
            status: Status::Fail,
            message: format!("TXT lookup failed: {e}"),
            details: vec![],
        },
    }
}

