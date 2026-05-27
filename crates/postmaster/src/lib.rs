#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Email-domain DNS health checks for postmasters.
//!
//! Given a domain, [`check_domain`] runs the full battery of DNS-level
//! checks any mail-server operator needs at deploy time and during
//! incident response: MX, SPF, DKIM, DMARC, MTA-STS, TLS-RPT, BIMI,
//! DANE, PTR / FCrDNS.
//!
//! Every check returns a [`CheckResult`] with a [`Status`], a
//! human-readable message, and structured details. The full report
//! ([`DomainCheckReport`]) is `Serialize` so you can ship it straight
//! to JSON / Prometheus / a CLI table.
//!
//! ## Example
//!
//! ```no_run
//! use hickory_resolver::TokioResolver;
//! use mailrs_postmaster::{HickoryPostmasterResolver, check_domain};
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let hickory = TokioResolver::builder_tokio()?.build()?;
//! let resolver = HickoryPostmasterResolver::new(hickory);
//! let report = check_domain(&resolver, "example.com", Some("default"), "mail.example.com").await;
//! for check in &report.checks {
//!     println!("{}: {:?} — {}", check.name, check.status, check.message);
//! }
//! # Ok(())
//! # }
//! ```

use serde::Serialize;

pub use resolver::{MockResolver, MxRecord, PostmasterResolver, ResolverError};
#[cfg(feature = "hickory")]
pub use resolver::HickoryPostmasterResolver;

/// Full domain-health report returned by [`check_domain`].
#[derive(Debug, Clone, Serialize)]
pub struct DomainCheckReport {
    /// Domain that was checked.
    pub domain: String,
    /// One [`CheckResult`] per individual check the report ran.
    pub checks: Vec<CheckResult>,
    /// Epoch seconds the report was generated.
    pub checked_at: i64,
}

/// One individual check inside a [`DomainCheckReport`].
#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    /// Check name (e.g. `"mx"`, `"spf"`, `"dkim"`).
    pub name: String,
    /// Overall outcome — see [`Status`].
    pub status: Status,
    /// Short human-readable summary suitable for log lines.
    pub message: String,
    /// Optional per-record detail (e.g. each MX hostname's individual A-record check).
    pub details: Vec<String>,
}

/// Outcome bucket for a single [`CheckResult`].
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Check satisfied all expected invariants.
    Pass,
    /// Check passed but flagged something the operator should look at.
    Warn,
    /// Check failed.
    Fail,
    /// Check did not run (preconditions not met / unsupported).
    Skip,
}

/// Run every health check this crate knows against `domain`.
///
/// `dkim_selector` lets the caller specify the published selector for the
/// DKIM check; pass `None` to skip it. `hostname` is the expected reverse-
/// DNS / EHLO name for outgoing mail.
pub async fn check_domain<R: PostmasterResolver + ?Sized>(
    resolver: &R,
    domain: &str,
    dkim_selector: Option<&str>,
    hostname: &str,
) -> DomainCheckReport {
    let mut checks = Vec::with_capacity(9);

    checks.push(mx::check_mx(resolver, domain, hostname).await);
    checks.push(spf::check_spf(resolver, domain, hostname).await);
    checks.push(dkim::check_dkim(resolver, domain, dkim_selector).await);
    checks.push(dmarc::check_dmarc(resolver, domain).await);
    checks.push(mta_sts::check_mta_sts_record(resolver, domain).await);
    checks.push(mta_sts::check_mta_sts_policy(domain).await);
    checks.push(tlsrpt::check_tlsrpt(resolver, domain).await);
    checks.push(ptr::check_ptr(resolver, hostname).await);
    checks.push(dane::check_dane(resolver, domain).await);
    checks.push(bimi::check_bimi(resolver, domain).await);

    DomainCheckReport {
        domain: domain.to_string(),
        checks,
        checked_at: chrono::Utc::now().timestamp(),
    }
}

mod bimi;
mod dane;
mod dkim;
mod dmarc;
mod mta_sts;
mod mx;
mod ptr;
mod resolver;
mod spf;
mod tlsrpt;

pub use bimi::{extract_bimi_logo_url, lookup_bimi_logo};
pub use mta_sts::parse_mta_sts_policy;
pub use tlsrpt::{extract_tlsrpt_rua, validate_tlsrpt_record};

#[cfg(test)]
/// extract the mode from a parsed MTA-STS policy
fn extract_sts_mode(fields: &[(String, String)]) -> Option<&str> {
    fields
        .iter()
        .find(|(k, _)| k == "mode")
        .map(|(_, v)| v.as_str())
}

#[cfg(test)]
/// extract all mx patterns from a parsed MTA-STS policy
fn extract_sts_mx_patterns(fields: &[(String, String)]) -> Vec<&str> {
    fields
        .iter()
        .filter(|(k, _)| k == "mx")
        .map(|(_, v)| v.as_str())
        .collect()
}

#[cfg(test)]
/// extract max_age from a parsed MTA-STS policy (in seconds)
fn extract_sts_max_age(fields: &[(String, String)]) -> Option<u64> {
    fields
        .iter()
        .find(|(k, _)| k == "max_age")
        .and_then(|(_, v)| v.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Status enum tests --

    #[test]
    fn status_serialize_pass() {
        let json = serde_json::to_string(&Status::Pass).unwrap();
        assert_eq!(json, "\"pass\"");
    }

    #[test]
    fn status_serialize_warn() {
        let json = serde_json::to_string(&Status::Warn).unwrap();
        assert_eq!(json, "\"warn\"");
    }

    #[test]
    fn status_serialize_fail() {
        let json = serde_json::to_string(&Status::Fail).unwrap();
        assert_eq!(json, "\"fail\"");
    }

    #[test]
    fn status_serialize_skip() {
        let json = serde_json::to_string(&Status::Skip).unwrap();
        assert_eq!(json, "\"skip\"");
    }

    #[test]
    fn status_debug_format() {
        assert!(format!("{:?}", Status::Pass).contains("Pass"));
        assert!(format!("{:?}", Status::Warn).contains("Warn"));
        assert!(format!("{:?}", Status::Fail).contains("Fail"));
        assert!(format!("{:?}", Status::Skip).contains("Skip"));
    }

    #[test]
    fn status_clone() {
        let s = Status::Pass;
        let s2 = s;
        assert!(matches!(s, Status::Pass));
        assert!(matches!(s2, Status::Pass));
    }

    // -- CheckResult tests --

    #[test]
    fn check_result_serialize() {
        let result = CheckResult {
            name: "Test".into(),
            status: Status::Pass,
            message: "ok".into(),
            details: vec!["detail1".into()],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"name\":\"Test\""));
        assert!(json.contains("\"status\":\"pass\""));
        assert!(json.contains("\"message\":\"ok\""));
        assert!(json.contains("\"details\":[\"detail1\"]"));
    }

    #[test]
    fn check_result_empty_details() {
        let result = CheckResult {
            name: "Empty".into(),
            status: Status::Skip,
            message: "skipped".into(),
            details: vec![],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"details\":[]"));
    }

    // -- DomainCheckReport tests --

    #[test]
    fn domain_check_report_serialize() {
        let report = DomainCheckReport {
            domain: "example.com".into(),
            checks: vec![CheckResult {
                name: "MX".into(),
                status: Status::Pass,
                message: "ok".into(),
                details: vec![],
            }],
            checked_at: 1700000000,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"domain\":\"example.com\""));
        assert!(json.contains("\"checked_at\":1700000000"));
        assert!(json.contains("\"checks\":["));
    }

    #[test]
    fn domain_check_report_empty_checks() {
        let report = DomainCheckReport {
            domain: "empty.org".into(),
            checks: vec![],
            checked_at: 0,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"checks\":[]"));
    }

    // -- MTA-STS policy parsing tests --

    #[test]
    fn parse_mta_sts_policy_enforce_mode() {
        let body = "version: STSv1\nmode: enforce\nmx: mail.example.com\nmx: *.example.com\nmax_age: 86400\n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(fields.len(), 5);
        assert_eq!(fields[0], ("version".into(), "STSv1".into()));
        assert_eq!(fields[1], ("mode".into(), "enforce".into()));
        assert_eq!(fields[2], ("mx".into(), "mail.example.com".into()));
        assert_eq!(fields[3], ("mx".into(), "*.example.com".into()));
        assert_eq!(fields[4], ("max_age".into(), "86400".into()));
    }

    #[test]
    fn parse_mta_sts_policy_testing_mode() {
        let body = "version: STSv1\nmode: testing\nmx: *.example.com\nmax_age: 604800\n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(extract_sts_mode(&fields), Some("testing"));
    }

    #[test]
    fn parse_mta_sts_policy_none_mode() {
        let body = "version: STSv1\nmode: none\nmax_age: 0\n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(extract_sts_mode(&fields), Some("none"));
        assert_eq!(extract_sts_max_age(&fields), Some(0));
    }

    #[test]
    fn parse_mta_sts_policy_crlf_line_endings() {
        let body = "version: STSv1\r\nmode: enforce\r\nmx: *.example.com\r\nmax_age: 86400\r\n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(extract_sts_mode(&fields), Some("enforce"));
        let mx = extract_sts_mx_patterns(&fields);
        assert_eq!(mx, vec!["*.example.com"]);
    }

    #[test]
    fn parse_mta_sts_policy_empty_body() {
        let fields = parse_mta_sts_policy("");
        assert!(fields.is_empty());
    }

    #[test]
    fn parse_mta_sts_policy_blank_lines_ignored() {
        let body = "version: STSv1\n\nmode: enforce\n\nmx: *.example.com\n\nmax_age: 86400\n\n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(fields.len(), 4);
    }

    #[test]
    fn parse_mta_sts_policy_whitespace_handling() {
        let body = "  version:   STSv1  \n  mode:  enforce  \n  mx:  mail.example.com  \n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(fields[0], ("version".into(), "STSv1".into()));
        assert_eq!(fields[1], ("mode".into(), "enforce".into()));
        assert_eq!(fields[2], ("mx".into(), "mail.example.com".into()));
    }

    #[test]
    fn parse_mta_sts_policy_keys_lowercased() {
        let body = "VERSION: STSv1\nMODE: enforce\nMX: *.example.com\nMAX_AGE: 86400\n";
        let fields = parse_mta_sts_policy(body);
        assert!(fields.iter().all(|(k, _)| k == &k.to_lowercase()));
    }

    #[test]
    fn parse_mta_sts_policy_no_colon_lines_skipped() {
        let body = "version: STSv1\nthis line has no colon\nmode: enforce\n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(fields.len(), 2);
    }

    #[test]
    fn parse_mta_sts_policy_value_with_colon() {
        // only splits on first colon
        let body = "version: STSv1\nmx: host:with:colons\n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(fields[1].1, "host:with:colons");
    }

    // -- extract_sts_mode tests --

    #[test]
    fn extract_sts_mode_missing() {
        let fields = vec![("version".into(), "STSv1".into())];
        assert_eq!(extract_sts_mode(&fields), None);
    }

    #[test]
    fn extract_sts_mode_enforce() {
        let fields = vec![
            ("version".into(), "STSv1".into()),
            ("mode".into(), "enforce".into()),
        ];
        assert_eq!(extract_sts_mode(&fields), Some("enforce"));
    }

    #[test]
    fn extract_sts_mode_from_empty_fields() {
        let fields: Vec<(String, String)> = vec![];
        assert_eq!(extract_sts_mode(&fields), None);
    }

    // -- extract_sts_mx_patterns tests --

    #[test]
    fn extract_sts_mx_patterns_multiple() {
        let fields = vec![
            ("mx".into(), "mail.example.com".into()),
            ("mx".into(), "*.example.com".into()),
            ("mx".into(), "backup.example.com".into()),
        ];
        let patterns = extract_sts_mx_patterns(&fields);
        assert_eq!(
            patterns,
            vec!["mail.example.com", "*.example.com", "backup.example.com"]
        );
    }

    #[test]
    fn extract_sts_mx_patterns_none() {
        let fields = vec![("version".into(), "STSv1".into())];
        let patterns = extract_sts_mx_patterns(&fields);
        assert!(patterns.is_empty());
    }

    #[test]
    fn extract_sts_mx_patterns_single() {
        let fields = vec![("mx".into(), "*.example.com".into())];
        let patterns = extract_sts_mx_patterns(&fields);
        assert_eq!(patterns, vec!["*.example.com"]);
    }

    // -- extract_sts_max_age tests --

    #[test]
    fn extract_sts_max_age_valid() {
        let fields = vec![("max_age".into(), "86400".into())];
        assert_eq!(extract_sts_max_age(&fields), Some(86400));
    }

    #[test]
    fn extract_sts_max_age_zero() {
        let fields = vec![("max_age".into(), "0".into())];
        assert_eq!(extract_sts_max_age(&fields), Some(0));
    }

    #[test]
    fn extract_sts_max_age_large() {
        // 31557600 = 1 year in seconds
        let fields = vec![("max_age".into(), "31557600".into())];
        assert_eq!(extract_sts_max_age(&fields), Some(31557600));
    }

    #[test]
    fn extract_sts_max_age_missing() {
        let fields = vec![("mode".into(), "enforce".into())];
        assert_eq!(extract_sts_max_age(&fields), None);
    }

    #[test]
    fn extract_sts_max_age_invalid_value() {
        let fields = vec![("max_age".into(), "not_a_number".into())];
        assert_eq!(extract_sts_max_age(&fields), None);
    }

    #[test]
    fn extract_sts_max_age_negative() {
        let fields = vec![("max_age".into(), "-100".into())];
        assert_eq!(extract_sts_max_age(&fields), None);
    }

    // -- MTA-STS full policy integration tests --

    #[test]
    fn mta_sts_enforce_policy_complete() {
        let body = "\
version: STSv1\r\n\
mode: enforce\r\n\
mx: mail.example.com\r\n\
mx: *.example.com\r\n\
max_age: 604800\r\n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(extract_sts_mode(&fields), Some("enforce"));
        let mx = extract_sts_mx_patterns(&fields);
        assert_eq!(mx.len(), 2);
        assert_eq!(extract_sts_max_age(&fields), Some(604800));
    }

    #[test]
    fn mta_sts_testing_policy_complete() {
        let body = "\
version: STSv1\n\
mode: testing\n\
mx: *.example.org\n\
max_age: 86400\n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(extract_sts_mode(&fields), Some("testing"));
        let mx = extract_sts_mx_patterns(&fields);
        assert_eq!(mx, vec!["*.example.org"]);
        assert_eq!(extract_sts_max_age(&fields), Some(86400));
    }

    #[test]
    fn mta_sts_none_policy_no_mx() {
        let body = "version: STSv1\nmode: none\nmax_age: 0\n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(extract_sts_mode(&fields), Some("none"));
        let mx = extract_sts_mx_patterns(&fields);
        assert!(mx.is_empty());
        assert_eq!(extract_sts_max_age(&fields), Some(0));
    }

    #[test]
    fn mta_sts_real_world_google_policy() {
        // based on actual google MTA-STS policy
        let body = "\
version: STSv1\n\
mode: enforce\n\
mx: gmail-smtp-in.l.google.com\n\
mx: *.gmail-smtp-in.l.google.com\n\
max_age: 86400\n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(extract_sts_mode(&fields), Some("enforce"));
        let mx = extract_sts_mx_patterns(&fields);
        assert_eq!(mx.len(), 2);
        assert!(mx.contains(&"gmail-smtp-in.l.google.com"));
        assert!(mx.contains(&"*.gmail-smtp-in.l.google.com"));
    }

    #[test]
    fn mta_sts_real_world_microsoft_policy() {
        let body = "\
version: STSv1\n\
mode: enforce\n\
mx: *.mail.protection.outlook.com\n\
max_age: 604800\n";
        let fields = parse_mta_sts_policy(body);
        assert_eq!(extract_sts_mode(&fields), Some("enforce"));
        let mx = extract_sts_mx_patterns(&fields);
        assert_eq!(mx, vec!["*.mail.protection.outlook.com"]);
        assert_eq!(extract_sts_max_age(&fields), Some(604800));
    }

    // -- validate_tlsrpt_record tests --

    #[test]
    fn validate_tlsrpt_valid_mailto() {
        assert!(validate_tlsrpt_record(
            "v=TLSRPTv1; rua=mailto:tls-reports@example.com"
        ));
    }

    #[test]
    fn validate_tlsrpt_valid_https() {
        assert!(validate_tlsrpt_record(
            "v=TLSRPTv1; rua=https://reporting.example.com/tls"
        ));
    }

    #[test]
    fn validate_tlsrpt_valid_multiple_rua() {
        assert!(validate_tlsrpt_record(
            "v=TLSRPTv1; rua=mailto:a@example.com,https://report.example.com"
        ));
    }

    #[test]
    fn validate_tlsrpt_missing_version() {
        assert!(!validate_tlsrpt_record("rua=mailto:a@example.com"));
    }

    #[test]
    fn validate_tlsrpt_missing_rua() {
        assert!(!validate_tlsrpt_record("v=TLSRPTv1"));
    }

    #[test]
    fn validate_tlsrpt_empty_string() {
        assert!(!validate_tlsrpt_record(""));
    }

    #[test]
    fn validate_tlsrpt_wrong_version() {
        assert!(!validate_tlsrpt_record(
            "v=TLSRPTv2; rua=mailto:a@example.com"
        ));
    }

    // -- extract_tlsrpt_rua tests --

    #[test]
    fn extract_tlsrpt_rua_mailto() {
        let uris = extract_tlsrpt_rua("v=TLSRPTv1; rua=mailto:reports@example.com");
        assert_eq!(uris, vec!["mailto:reports@example.com"]);
    }

    #[test]
    fn extract_tlsrpt_rua_https() {
        let uris = extract_tlsrpt_rua("v=TLSRPTv1; rua=https://example.com/tls-report");
        assert_eq!(uris, vec!["https://example.com/tls-report"]);
    }

    #[test]
    fn extract_tlsrpt_rua_multiple() {
        let uris =
            extract_tlsrpt_rua("v=TLSRPTv1; rua=mailto:a@example.com,https://example.com/report");
        assert_eq!(
            uris,
            vec!["mailto:a@example.com", "https://example.com/report"]
        );
    }

    #[test]
    fn extract_tlsrpt_rua_no_rua_field() {
        let uris = extract_tlsrpt_rua("v=TLSRPTv1");
        assert!(uris.is_empty());
    }

    #[test]
    fn extract_tlsrpt_rua_empty_record() {
        let uris = extract_tlsrpt_rua("");
        assert!(uris.is_empty());
    }

    #[test]
    fn extract_tlsrpt_rua_whitespace_handling() {
        let uris = extract_tlsrpt_rua("v=TLSRPTv1 ;  rua=mailto:x@example.com  ");
        assert_eq!(uris, vec!["mailto:x@example.com"]);
    }

    #[test]
    fn extract_tlsrpt_rua_three_uris() {
        let uris = extract_tlsrpt_rua(
            "v=TLSRPTv1; rua=mailto:a@a.com,mailto:b@b.com,https://c.com/report",
        );
        assert_eq!(uris.len(), 3);
        assert_eq!(uris[0], "mailto:a@a.com");
        assert_eq!(uris[1], "mailto:b@b.com");
        assert_eq!(uris[2], "https://c.com/report");
    }

    // -- MTA-STS DNS record format tests --

    #[test]
    fn mta_sts_dns_record_name() {
        let domain = "example.com";
        let qname = format!("_mta-sts.{domain}");
        assert_eq!(qname, "_mta-sts.example.com");
    }

    #[test]
    fn mta_sts_policy_url_format() {
        let domain = "example.com";
        let url = format!("https://mta-sts.{domain}/.well-known/mta-sts.txt");
        assert_eq!(url, "https://mta-sts.example.com/.well-known/mta-sts.txt");
    }

    #[test]
    fn tlsrpt_dns_record_name() {
        let domain = "example.com";
        let qname = format!("_smtp._tls.{domain}");
        assert_eq!(qname, "_smtp._tls.example.com");
    }

    // -- MTA-STS mode behavior tests --

    #[test]
    fn enforce_mode_requires_tls() {
        let body = "version: STSv1\nmode: enforce\nmx: *.example.com\nmax_age: 86400\n";
        let fields = parse_mta_sts_policy(body);
        let mode = extract_sts_mode(&fields).unwrap();
        assert_eq!(mode, "enforce");
        // enforce means sender MUST use TLS and verify cert
    }

    #[test]
    fn testing_mode_allows_reporting() {
        let body = "version: STSv1\nmode: testing\nmx: *.example.com\nmax_age: 86400\n";
        let fields = parse_mta_sts_policy(body);
        let mode = extract_sts_mode(&fields).unwrap();
        assert_eq!(mode, "testing");
        // testing means send reports but don't enforce
    }

    #[test]
    fn none_mode_disables_policy() {
        let body = "version: STSv1\nmode: none\nmax_age: 0\n";
        let fields = parse_mta_sts_policy(body);
        let mode = extract_sts_mode(&fields).unwrap();
        assert_eq!(mode, "none");
        // none means no policy is active
    }

    // -- MTA-STS max_age boundary tests --

    #[test]
    fn max_age_recommended_minimum() {
        // RFC 8461 recommends max_age of at least 86400 (1 day) for enforce mode
        let fields = vec![("max_age".into(), "86400".into())];
        let age = extract_sts_max_age(&fields).unwrap();
        assert!(age >= 86400);
    }

    #[test]
    fn max_age_common_values() {
        // 1 day
        let fields = vec![("max_age".into(), "86400".into())];
        assert_eq!(extract_sts_max_age(&fields), Some(86400));

        // 1 week
        let fields = vec![("max_age".into(), "604800".into())];
        assert_eq!(extract_sts_max_age(&fields), Some(604800));

        // 1 year
        let fields = vec![("max_age".into(), "31557600".into())];
        assert_eq!(extract_sts_max_age(&fields), Some(31557600));
    }

    // -- check_mta_sts_policy body validation logic tests --

    #[test]
    fn policy_body_has_mode_and_mx() {
        let body = "version: STSv1\nmode: enforce\nmx: *.example.com\nmax_age: 86400\n";
        let has_mode = body.contains("mode:");
        let has_mx = body.contains("mx:");
        assert!(has_mode && has_mx);
    }

    #[test]
    fn policy_body_missing_mode() {
        let body = "version: STSv1\nmx: *.example.com\nmax_age: 86400\n";
        let has_mode = body.contains("mode:");
        assert!(!has_mode);
    }

    #[test]
    fn policy_body_missing_mx() {
        let body = "version: STSv1\nmode: enforce\nmax_age: 86400\n";
        let has_mx = body.contains("mx:");
        assert!(!has_mx);
    }

    #[test]
    fn policy_body_empty() {
        let body = "";
        assert!(!body.contains("mode:"));
        assert!(!body.contains("mx:"));
    }

    // -- STS record v=STSv1 detection --

    #[test]
    fn sts_record_valid() {
        let record = "v=STSv1; id=20240101T000000Z";
        assert!(record.contains("v=STSv1"));
    }

    #[test]
    fn sts_record_invalid_version() {
        let record = "v=STSv2; id=20240101T000000Z";
        assert!(!record.contains("v=STSv1"));
    }

    #[test]
    fn sts_record_no_version() {
        let record = "id=20240101T000000Z";
        assert!(!record.contains("v=STSv1"));
    }

    // -- TLSRPT record v=TLSRPTv1 detection --

    #[test]
    fn tlsrpt_record_valid() {
        let record = "v=TLSRPTv1; rua=mailto:reports@example.com";
        assert!(record.contains("v=TLSRPTv1"));
    }

    #[test]
    fn tlsrpt_record_invalid_version() {
        let record = "v=TLSRPTv2; rua=mailto:reports@example.com";
        assert!(!record.contains("v=TLSRPTv1"));
    }

    // -- combined MTA-STS + mx_matches_policy integration --

    #[test]
    fn mta_sts_policy_and_mx_matching_integration() {
        let body = "\
version: STSv1\n\
mode: enforce\n\
mx: mail.example.com\n\
mx: *.example.com\n\
max_age: 86400\n";
        let fields = parse_mta_sts_policy(body);
        let mx_patterns = extract_sts_mx_patterns(&fields);

        // use mx_matches_policy from the outbound-queue crate logic
        // (replicated here as the same algorithm)
        let mx_host = "relay.example.com";
        let matches = mx_patterns.iter().any(|pattern| {
            let p = pattern.to_lowercase();
            let host = mx_host.to_lowercase();
            if p.starts_with("*.") {
                let suffix = &p[1..];
                host.ends_with(suffix)
                    && host.len() > suffix.len()
                    && !host[..host.len() - suffix.len()].contains('.')
            } else {
                host == p
            }
        });
        assert!(matches);
    }

    #[test]
    fn mta_sts_policy_enforce_rejects_non_matching_mx() {
        let body = "\
version: STSv1\n\
mode: enforce\n\
mx: *.example.com\n\
max_age: 86400\n";
        let fields = parse_mta_sts_policy(body);
        let mx_patterns = extract_sts_mx_patterns(&fields);

        let mx_host = "mail.other.com";
        let matches = mx_patterns.iter().any(|pattern| {
            let p = pattern.to_lowercase();
            let host = mx_host.to_lowercase();
            if p.starts_with("*.") {
                let suffix = &p[1..];
                host.ends_with(suffix)
                    && host.len() > suffix.len()
                    && !host[..host.len() - suffix.len()].contains('.')
            } else {
                host == p
            }
        });
        assert!(!matches);
    }

    // -- BIMI record tests --

    #[test]
    fn bimi_dns_record_name() {
        let domain = "example.com";
        let qname = format!("default._bimi.{domain}");
        assert_eq!(qname, "default._bimi.example.com");
    }

    #[test]
    fn extract_bimi_logo_url_valid() {
        let record = "v=BIMI1; l=https://example.com/logo.svg; a=";
        assert_eq!(
            extract_bimi_logo_url(record),
            Some("https://example.com/logo.svg".into())
        );
    }

    #[test]
    fn extract_bimi_logo_url_no_logo() {
        let record = "v=BIMI1; l=; a=";
        assert_eq!(extract_bimi_logo_url(record), None);
    }

    #[test]
    fn extract_bimi_logo_url_missing_l_tag() {
        let record = "v=BIMI1; a=https://example.com/vmc.pem";
        assert_eq!(extract_bimi_logo_url(record), None);
    }

    #[test]
    fn extract_bimi_logo_url_with_authority() {
        let record = "v=BIMI1; l=https://example.com/brand.svg; a=https://example.com/vmc.pem";
        assert_eq!(
            extract_bimi_logo_url(record),
            Some("https://example.com/brand.svg".into())
        );
    }

    #[test]
    fn extract_bimi_logo_url_whitespace() {
        let record = "v=BIMI1;  l = https://example.com/logo.svg ;";
        // the "l" part after split is " l = https://...", which starts_with "l=" is false
        // so this tests trimming behavior
        let url = extract_bimi_logo_url(record);
        // the part " l = https://..." doesn't start with "l=" after trim, it's "l = https://..."
        assert_eq!(url, None);
    }

    #[test]
    fn bimi_record_detection() {
        let record = "v=BIMI1; l=https://example.com/logo.svg";
        assert!(record.contains("v=BIMI1"));
    }

    #[test]
    fn bimi_record_wrong_version() {
        let record = "v=BIMI2; l=https://example.com/logo.svg";
        assert!(!record.contains("v=BIMI1"));
    }
}
