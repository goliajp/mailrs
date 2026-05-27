//! Per-check submodule (see lib.rs for the dispatcher).

use super::resolver::PostmasterResolver;
use super::{CheckResult, Status};

pub(super) async fn check_dane<R: PostmasterResolver + ?Sized>(
    resolver: &R,
    domain: &str,
) -> CheckResult {
    // look up MX first, then check TLSA for port 25 on first MX
    let mx_host = match resolver.mx_lookup(domain).await {
        Ok(mut records) => {
            records.sort_by_key(|mx| mx.preference);
            records.into_iter().next().map(|mx| mx.exchange)
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
    match resolver.tlsa_lookup(&qname).await {
        Ok(entries) => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::{MockResolver, MxRecord};

    #[tokio::test]
    async fn no_mx_yields_skip() {
        let r = MockResolver::new();
        let res = check_dane(&r, "example.com").await;
        assert!(matches!(res.status, Status::Skip));
        assert!(res.message.contains("no MX"));
    }

    #[tokio::test]
    async fn mx_but_no_tlsa_yields_skip() {
        let r = MockResolver::new().with_mx(
            "example.com",
            vec![MxRecord {
                preference: 10,
                exchange: "mx.example.com".into(),
            }],
        );
        let res = check_dane(&r, "example.com").await;
        assert!(matches!(res.status, Status::Skip));
        assert!(res.message.contains("DANE not configured") || res.message.contains("no TLSA"));
    }

    #[tokio::test]
    async fn mx_with_tlsa_yields_pass() {
        let r = MockResolver::new()
            .with_mx(
                "example.com",
                vec![MxRecord {
                    preference: 10,
                    exchange: "mx.example.com".into(),
                }],
            )
            .with_tlsa("_25._tcp.mx.example.com", vec!["3 1 1 abc123".into()]);
        let res = check_dane(&r, "example.com").await;
        assert!(matches!(res.status, Status::Pass));
        assert_eq!(res.details, vec!["3 1 1 abc123"]);
    }

    #[tokio::test]
    async fn lowest_preference_mx_used() {
        // first MX in input is higher preference (less preferred) — should
        // pick the lower-preference one for the TLSA lookup
        let r = MockResolver::new()
            .with_mx(
                "example.com",
                vec![
                    MxRecord { preference: 30, exchange: "third.example.com".into() },
                    MxRecord { preference: 10, exchange: "primary.example.com".into() },
                    MxRecord { preference: 20, exchange: "second.example.com".into() },
                ],
            )
            .with_tlsa("_25._tcp.primary.example.com", vec!["3 1 1 deadbeef".into()]);
        let res = check_dane(&r, "example.com").await;
        assert!(matches!(res.status, Status::Pass));
        assert_eq!(res.details, vec!["3 1 1 deadbeef"]);
    }
}
