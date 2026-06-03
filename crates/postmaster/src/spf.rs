//! Per-check submodule (see lib.rs for the dispatcher).

use std::net::IpAddr;

use super::resolver::PostmasterResolver;
use super::{CheckResult, Status};

pub(super) async fn check_spf<R: PostmasterResolver + ?Sized>(
    resolver: &R,
    domain: &str,
    hostname: &str,
) -> CheckResult {
    // resolve our hostname to IPs for SPF inclusion check
    let our_ips: Vec<IpAddr> = resolver.ip_lookup(hostname).await.unwrap_or_default();

    match resolver.txt_lookup(domain).await {
        Ok(records) => {
            let spf_records: Vec<String> = records
                .into_iter()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::MockResolver;

    #[tokio::test]
    async fn no_spf_yields_fail() {
        let r = MockResolver::new();
        let res = check_spf(&r, "example.com", "mail.example.com").await;
        assert!(matches!(res.status, Status::Fail));
    }

    #[tokio::test]
    async fn spf_including_hostname_yields_pass() {
        let r = MockResolver::new().with_txt(
            "example.com",
            vec!["v=spf1 mx a:mail.example.com -all".into()],
        );
        let res = check_spf(&r, "example.com", "mail.example.com").await;
        assert!(matches!(res.status, Status::Pass));
        assert!(res.message.contains("strict"));
    }

    #[tokio::test]
    async fn spf_not_including_hostname_yields_warn() {
        let r = MockResolver::new()
            .with_txt("example.com", vec!["v=spf1 include:other.com -all".into()]);
        let res = check_spf(&r, "example.com", "mail.example.com").await;
        assert!(matches!(res.status, Status::Warn));
    }

    #[tokio::test]
    async fn soft_fail_policy_recognized() {
        let r = MockResolver::new()
            .with_txt("example.com", vec!["v=spf1 mx ~all".into()])
            .with_mx("example.com", vec![]);
        let res = check_spf(&r, "example.com", "anything").await;
        assert!(res.message.contains("soft fail"));
    }

    #[tokio::test]
    async fn dangerous_pass_all_recognized() {
        let r = MockResolver::new().with_txt("example.com", vec!["v=spf1 +all".into()]);
        let res = check_spf(&r, "example.com", "mail.example.com").await;
        assert!(res.message.contains("+all"));
    }

    #[tokio::test]
    async fn multiple_spf_yields_warn() {
        let r = MockResolver::new().with_txt(
            "example.com",
            vec!["v=spf1 mx -all".into(), "v=spf1 a -all".into()],
        );
        let res = check_spf(&r, "example.com", "mail.example.com").await;
        assert!(matches!(res.status, Status::Warn));
        assert!(res.message.contains("multiple"));
    }

    #[tokio::test]
    async fn ip_match_via_ip_lookup_yields_pass() {
        use std::net::{IpAddr, Ipv4Addr};
        let ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 5));
        let r = MockResolver::new()
            .with_txt("example.com", vec!["v=spf1 ip4:203.0.113.5 -all".into()])
            .with_ip("mail.example.com", vec![ip]);
        let res = check_spf(&r, "example.com", "mail.example.com").await;
        assert!(matches!(res.status, Status::Pass));
    }
}
