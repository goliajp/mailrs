//! Per-check submodule (see lib.rs for the dispatcher).

use super::resolver::PostmasterResolver;
use super::{CheckResult, Status};

pub(super) async fn check_bimi<R: PostmasterResolver + ?Sized>(
    resolver: &R,
    domain: &str,
) -> CheckResult {
    let qname = format!("default._bimi.{domain}");
    match resolver.txt_lookup(&qname).await {
        Ok(records) => {
            let bimi_records: Vec<String> = records
                .into_iter()
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
pub async fn lookup_bimi_logo<R: PostmasterResolver + ?Sized>(
    resolver: &R,
    domain: &str,
) -> Option<String> {
    let qname = format!("default._bimi.{domain}");
    let records = resolver.txt_lookup(&qname).await.ok()?;
    records
        .into_iter()
        .find(|txt| txt.contains("v=BIMI1"))
        .and_then(|rec| extract_bimi_logo_url(&rec))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::MockResolver;

    #[tokio::test]
    async fn no_bimi_record_yields_skip() {
        let r = MockResolver::new();
        let res = check_bimi(&r, "example.com").await;
        assert!(matches!(res.status, Status::Skip));
        assert!(res.message.contains("no BIMI record"));
    }

    #[tokio::test]
    async fn bimi_record_with_logo_yields_pass() {
        let r = MockResolver::new().with_txt(
            "default._bimi.example.com",
            vec!["v=BIMI1; l=https://example.com/logo.svg".into()],
        );
        let res = check_bimi(&r, "example.com").await;
        assert!(matches!(res.status, Status::Pass));
        assert_eq!(res.details, vec!["v=BIMI1; l=https://example.com/logo.svg"]);
    }

    #[tokio::test]
    async fn bimi_record_without_logo_yields_warn() {
        let r = MockResolver::new()
            .with_txt("default._bimi.example.com", vec!["v=BIMI1; a=https://example.com/cert.pem".into()]);
        let res = check_bimi(&r, "example.com").await;
        assert!(matches!(res.status, Status::Warn));
    }

    #[tokio::test]
    async fn non_bimi_txt_records_filtered_out() {
        let r = MockResolver::new().with_txt(
            "default._bimi.example.com",
            vec!["foo=bar".into(), "v=DMARC1; p=reject".into()],
        );
        let res = check_bimi(&r, "example.com").await;
        assert!(matches!(res.status, Status::Skip));
    }

    #[tokio::test]
    async fn lookup_bimi_logo_returns_url() {
        let r = MockResolver::new().with_txt(
            "default._bimi.example.com",
            vec!["v=BIMI1; l=https://x.example/logo.svg".into()],
        );
        let url = lookup_bimi_logo(&r, "example.com").await;
        assert_eq!(url, Some("https://x.example/logo.svg".into()));
    }

    #[tokio::test]
    async fn lookup_bimi_logo_returns_none_when_missing() {
        let r = MockResolver::new();
        let url = lookup_bimi_logo(&r, "example.com").await;
        assert_eq!(url, None);
    }

    #[test]
    fn extract_logo_url_present() {
        assert_eq!(
            extract_bimi_logo_url("v=BIMI1; l=https://example.com/logo.svg"),
            Some("https://example.com/logo.svg".into())
        );
    }

    #[test]
    fn extract_logo_url_empty_returns_none() {
        assert_eq!(extract_bimi_logo_url("v=BIMI1; l="), None);
    }

    #[test]
    fn extract_logo_url_missing_returns_none() {
        assert_eq!(extract_bimi_logo_url("v=BIMI1"), None);
    }

    #[test]
    fn extract_logo_url_with_whitespace() {
        assert_eq!(
            extract_bimi_logo_url("v=BIMI1 ; l=  https://x/y "),
            Some("https://x/y".into())
        );
    }
}
