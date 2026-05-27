//! Per-check submodule (see lib.rs for the dispatcher).

use super::resolver::PostmasterResolver;
use super::{CheckResult, Status};

pub(super) async fn check_mta_sts_record<R: PostmasterResolver + ?Sized>(
    resolver: &R,
    domain: &str,
) -> CheckResult {
    let qname = format!("_mta-sts.{domain}");
    match resolver.txt_lookup(&qname).await {
        Ok(records) => {
            let sts_records: Vec<String> = records
                .into_iter()
                .filter(|txt| txt.contains("v=STSv1"))
                .collect();
            if sts_records.is_empty() {
                CheckResult {
                    name: "MTA-STS Record".into(),
                    status: Status::Warn,
                    message: "no MTA-STS TXT record found".into(),
                    details: vec![],
                }
            } else {
                CheckResult {
                    name: "MTA-STS Record".into(),
                    status: Status::Pass,
                    message: "MTA-STS TXT record found".into(),
                    details: sts_records,
                }
            }
        }
        Err(_) => CheckResult {
            name: "MTA-STS Record".into(),
            status: Status::Warn,
            message: "no MTA-STS TXT record found".into(),
            details: vec![],
        },
    }
}

pub(super) async fn check_mta_sts_policy(domain: &str) -> CheckResult {
    let url = format!("https://mta-sts.{domain}/.well-known/mta-sts.txt");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build();
    let Ok(client) = client else {
        return CheckResult {
            name: "MTA-STS Policy".into(),
            status: Status::Skip,
            message: "HTTP client error".into(),
            details: vec![],
        };
    };
    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => match resp.text().await {
            Ok(body) => {
                let has_mode = body.contains("mode:");
                let has_mx = body.contains("mx:");
                let status = if has_mode && has_mx {
                    Status::Pass
                } else {
                    Status::Warn
                };
                CheckResult {
                    name: "MTA-STS Policy".into(),
                    status,
                    message: format!("policy fetched from {url}"),
                    details: body.lines().map(|l| l.to_string()).collect(),
                }
            }
            Err(e) => CheckResult {
                name: "MTA-STS Policy".into(),
                status: Status::Warn,
                message: format!("failed to read policy body: {e}"),
                details: vec![],
            },
        },
        Ok(resp) => CheckResult {
            name: "MTA-STS Policy".into(),
            status: Status::Warn,
            message: format!("policy endpoint returned HTTP {}", resp.status()),
            details: vec![],
        },
        Err(e) => CheckResult {
            name: "MTA-STS Policy".into(),
            status: Status::Warn,
            message: format!("could not reach MTA-STS policy: {e}"),
            details: vec![],
        },
    }
}

/// Parse an MTA-STS policy body and extract key-value fields.
///
/// Returns a `Vec<(key, value)>` of every non-blank line; handles both
/// LF and CRLF line endings. Keys are lower-cased; values are trimmed.
/// Lines without a colon are skipped.
pub fn parse_mta_sts_policy(body: &str) -> Vec<(String, String)> {
    body.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            let (key, value) = trimmed.split_once(':')?;
            Some((key.trim().to_lowercase(), value.trim().to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::MockResolver;

    #[tokio::test]
    async fn no_record_yields_warn() {
        let r = MockResolver::new();
        let res = check_mta_sts_record(&r, "example.com").await;
        assert!(matches!(res.status, Status::Warn));
    }

    #[tokio::test]
    async fn valid_record_yields_pass() {
        let r = MockResolver::new()
            .with_txt("_mta-sts.example.com", vec!["v=STSv1; id=20260527T0001".into()]);
        let res = check_mta_sts_record(&r, "example.com").await;
        assert!(matches!(res.status, Status::Pass));
    }

    #[tokio::test]
    async fn unrelated_txt_filtered_out() {
        let r = MockResolver::new()
            .with_txt("_mta-sts.example.com", vec!["something else".into()]);
        let res = check_mta_sts_record(&r, "example.com").await;
        assert!(matches!(res.status, Status::Warn));
    }

    #[test]
    fn parse_simple_policy() {
        let body = "version: STSv1\nmode: enforce\nmx: mail.example.com\nmax_age: 86400";
        let kv = parse_mta_sts_policy(body);
        assert_eq!(kv.len(), 4);
        assert_eq!(kv[0], ("version".into(), "STSv1".into()));
        assert_eq!(kv[1], ("mode".into(), "enforce".into()));
        assert_eq!(kv[2], ("mx".into(), "mail.example.com".into()));
    }

    #[test]
    fn parse_skips_blank_lines() {
        let body = "version: STSv1\n\nmode: testing\n";
        let kv = parse_mta_sts_policy(body);
        assert_eq!(kv.len(), 2);
    }

    #[test]
    fn parse_lowercases_keys() {
        let body = "VERSION: STSv1\nMode: enforce";
        let kv = parse_mta_sts_policy(body);
        assert_eq!(kv[0].0, "version");
        assert_eq!(kv[1].0, "mode");
    }

    #[test]
    fn parse_skips_lines_without_colon() {
        let body = "version: STSv1\nignored\nmode: enforce";
        let kv = parse_mta_sts_policy(body);
        assert_eq!(kv.len(), 2);
    }
}
