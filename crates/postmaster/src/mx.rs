//! Per-check submodule (see lib.rs for the dispatcher).

use super::resolver::PostmasterResolver;
use super::{CheckResult, Status};

pub(super) async fn check_mx<R: PostmasterResolver + ?Sized>(
    resolver: &R,
    domain: &str,
    hostname: &str,
) -> CheckResult {
    match resolver.mx_lookup(domain).await {
        Ok(mxs) => {
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
            let points_to_us = mxs
                .iter()
                .any(|mx| mx.exchange.eq_ignore_ascii_case(hostname));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::{MockResolver, MxRecord};

    #[tokio::test]
    async fn no_mx_yields_fail() {
        let r = MockResolver::new();
        let res = check_mx(&r, "example.com", "mail.example.com").await;
        assert!(matches!(res.status, Status::Fail));
    }

    #[tokio::test]
    async fn mx_pointing_to_us_yields_pass() {
        let r = MockResolver::new().with_mx(
            "example.com",
            vec![MxRecord {
                preference: 10,
                exchange: "mail.example.com".into(),
            }],
        );
        let res = check_mx(&r, "example.com", "mail.example.com").await;
        assert!(matches!(res.status, Status::Pass));
        assert_eq!(res.details, vec!["mail.example.com (priority 10)"]);
    }

    #[tokio::test]
    async fn mx_not_pointing_to_us_yields_warn() {
        let r = MockResolver::new().with_mx(
            "example.com",
            vec![MxRecord {
                preference: 10,
                exchange: "other.example.com".into(),
            }],
        );
        let res = check_mx(&r, "example.com", "mail.example.com").await;
        assert!(matches!(res.status, Status::Warn));
    }

    #[tokio::test]
    async fn hostname_case_insensitive() {
        let r = MockResolver::new().with_mx(
            "example.com",
            vec![MxRecord {
                preference: 10,
                exchange: "Mail.Example.Com".into(),
            }],
        );
        let res = check_mx(&r, "example.com", "mail.example.com").await;
        assert!(matches!(res.status, Status::Pass));
    }
}
