//! Per-check submodule (see lib.rs for the dispatcher).

use super::resolver::PostmasterResolver;
use super::{CheckResult, Status};

pub(super) async fn check_ptr<R: PostmasterResolver + ?Sized>(
    resolver: &R,
    hostname: &str,
) -> CheckResult {
    // resolve hostname to IP, then reverse lookup
    let ip = match resolver.ip_lookup(hostname).await {
        Ok(ips) => ips.into_iter().next(),
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
        Ok(ptrs) => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::MockResolver;
    use std::net::{IpAddr, Ipv4Addr};

    #[tokio::test]
    async fn no_a_record_yields_fail() {
        let r = MockResolver::new();
        let res = check_ptr(&r, "mail.example.com").await;
        assert!(matches!(res.status, Status::Fail));
    }

    #[tokio::test]
    async fn ptr_matches_hostname_yields_pass() {
        let ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1));
        let r = MockResolver::new()
            .with_ip("mail.example.com", vec![ip])
            .with_reverse(ip, vec!["mail.example.com.".into()]);
        let res = check_ptr(&r, "mail.example.com").await;
        assert!(matches!(res.status, Status::Pass));
    }

    #[tokio::test]
    async fn ptr_does_not_match_yields_warn() {
        let ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 2));
        let r = MockResolver::new()
            .with_ip("mail.example.com", vec![ip])
            .with_reverse(ip, vec!["other.example.com.".into()]);
        let res = check_ptr(&r, "mail.example.com").await;
        assert!(matches!(res.status, Status::Warn));
    }

    #[tokio::test]
    async fn reverse_lookup_failure_yields_warn() {
        let ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 3));
        let r = MockResolver::new().with_ip("mail.example.com", vec![ip]);
        let res = check_ptr(&r, "mail.example.com").await;
        assert!(matches!(res.status, Status::Warn));
    }
}
