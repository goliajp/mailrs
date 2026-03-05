use std::fmt::Write;

/// individual authentication result for Auth-Results header
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthResult {
    pub method: String,
    pub result: String,
    /// optional reason (e.g. "policy=reject")
    pub reason: Option<String>,
}

/// format RFC 8601 Authentication-Results header value
pub fn format_auth_results(hostname: &str, results: &[AuthResult]) -> String {
    let mut buf = String::new();
    write!(buf, "{hostname}").unwrap();

    if results.is_empty() {
        buf.push_str("; none");
        return buf;
    }

    for r in results {
        write!(buf, ";\r\n\t{}={}", r.method, r.result).unwrap();
        if let Some(ref reason) = r.reason {
            write!(buf, " reason=\"{reason}\"").unwrap();
        }
    }
    buf
}

/// format the full header line including field name
pub fn format_auth_results_header(hostname: &str, results: &[AuthResult]) -> String {
    format!(
        "Authentication-Results: {}\r\n",
        format_auth_results(hostname, results)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_pass() {
        let results = vec![
            AuthResult {
                method: "spf".into(),
                result: "pass".into(),
                reason: None,
            },
            AuthResult {
                method: "dkim".into(),
                result: "pass".into(),
                reason: None,
            },
            AuthResult {
                method: "dmarc".into(),
                result: "pass".into(),
                reason: None,
            },
        ];
        let header = format_auth_results("mx.example.com", &results);
        assert!(header.starts_with("mx.example.com;"));
        assert!(header.contains("spf=pass"));
        assert!(header.contains("dkim=pass"));
        assert!(header.contains("dmarc=pass"));
    }

    #[test]
    fn spf_fail() {
        let results = vec![AuthResult {
            method: "spf".into(),
            result: "fail".into(),
            reason: Some("mechanism -all matched".into()),
        }];
        let header = format_auth_results("mx.example.com", &results);
        assert!(header.contains("spf=fail"));
        assert!(header.contains("reason=\"mechanism -all matched\""));
    }

    #[test]
    fn dkim_fail() {
        let results = vec![AuthResult {
            method: "dkim".into(),
            result: "fail".into(),
            reason: Some("body hash mismatch".into()),
        }];
        let header = format_auth_results("mx.example.com", &results);
        assert!(header.contains("dkim=fail"));
        assert!(header.contains("reason=\"body hash mismatch\""));
    }

    #[test]
    fn no_results() {
        let header = format_auth_results("mx.example.com", &[]);
        assert_eq!(header, "mx.example.com; none");
    }

    #[test]
    fn full_header_format() {
        let results = vec![AuthResult {
            method: "spf".into(),
            result: "pass".into(),
            reason: None,
        }];
        let header = format_auth_results_header("mx.example.com", &results);
        assert!(header.starts_with("Authentication-Results: mx.example.com;"));
        assert!(header.ends_with("\r\n"));
    }

    // --- additional DMARC-related auth_results tests ---

    #[test]
    fn dmarc_fail_with_policy_reason() {
        let results = vec![AuthResult {
            method: "dmarc".into(),
            result: "fail".into(),
            reason: Some("policy=reject".into()),
        }];
        let header = format_auth_results("mx.example.com", &results);
        assert!(header.contains("dmarc=fail"));
        assert!(header.contains("reason=\"policy=reject\""));
    }

    #[test]
    fn dmarc_quarantine_reason() {
        let results = vec![AuthResult {
            method: "dmarc".into(),
            result: "fail".into(),
            reason: Some("policy=quarantine".into()),
        }];
        let header = format_auth_results("mx.example.com", &results);
        assert!(header.contains("reason=\"policy=quarantine\""));
    }

    #[test]
    fn dmarc_none_reason() {
        let results = vec![AuthResult {
            method: "dmarc".into(),
            result: "fail".into(),
            reason: Some("policy=none".into()),
        }];
        let header = format_auth_results("mx.example.com", &results);
        assert!(header.contains("reason=\"policy=none\""));
    }

    #[test]
    fn full_pipeline_auth_results() {
        let results = vec![
            AuthResult { method: "spf".into(), result: "pass".into(), reason: None },
            AuthResult { method: "dkim".into(), result: "pass".into(), reason: None },
            AuthResult { method: "arc".into(), result: "none".into(), reason: None },
            AuthResult { method: "dmarc".into(), result: "pass".into(), reason: None },
        ];
        let header = format_auth_results("mx.mail.com", &results);
        assert!(header.starts_with("mx.mail.com;"));
        // all four methods present
        assert!(header.contains("spf=pass"));
        assert!(header.contains("dkim=pass"));
        assert!(header.contains("arc=none"));
        assert!(header.contains("dmarc=pass"));
    }

    #[test]
    fn all_fail_auth_results() {
        let results = vec![
            AuthResult { method: "spf".into(), result: "fail".into(), reason: Some("mechanism -all".into()) },
            AuthResult { method: "dkim".into(), result: "fail".into(), reason: Some("body hash mismatch".into()) },
            AuthResult { method: "arc".into(), result: "fail".into(), reason: None },
            AuthResult { method: "dmarc".into(), result: "fail".into(), reason: Some("policy=reject".into()) },
        ];
        let header = format_auth_results("mx.mail.com", &results);
        assert!(header.contains("spf=fail"));
        assert!(header.contains("dkim=fail"));
        assert!(header.contains("arc=fail"));
        assert!(header.contains("dmarc=fail"));
        assert!(header.contains("reason=\"policy=reject\""));
    }

    #[test]
    fn multiple_methods_folding() {
        // each result should be on its own folded line
        let results = vec![
            AuthResult { method: "spf".into(), result: "pass".into(), reason: None },
            AuthResult { method: "dmarc".into(), result: "pass".into(), reason: None },
        ];
        let header = format_auth_results("mx.example.com", &results);
        // should have CRLF+tab folding
        assert!(header.contains(";\r\n\t"));
    }

    #[test]
    fn auth_result_equality() {
        let a = AuthResult { method: "dmarc".into(), result: "pass".into(), reason: None };
        let b = AuthResult { method: "dmarc".into(), result: "pass".into(), reason: None };
        assert_eq!(a, b);
    }

    #[test]
    fn auth_result_inequality() {
        let a = AuthResult { method: "dmarc".into(), result: "pass".into(), reason: None };
        let b = AuthResult { method: "dmarc".into(), result: "fail".into(), reason: None };
        assert_ne!(a, b);
    }

    #[test]
    fn auth_result_clone() {
        let original = AuthResult {
            method: "dmarc".into(),
            result: "fail".into(),
            reason: Some("policy=reject".into()),
        };
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn auth_result_debug() {
        let r = AuthResult { method: "dmarc".into(), result: "pass".into(), reason: None };
        let debug = format!("{:?}", r);
        assert!(debug.contains("dmarc"));
        assert!(debug.contains("pass"));
    }

    #[test]
    fn single_dmarc_result_header_format() {
        let results = vec![AuthResult {
            method: "dmarc".into(),
            result: "pass".into(),
            reason: None,
        }];
        let header = format_auth_results_header("mx.example.com", &results);
        assert!(header.starts_with("Authentication-Results: "));
        assert!(header.contains("dmarc=pass"));
        assert!(header.ends_with("\r\n"));
    }

    #[test]
    fn no_results_header_shows_none() {
        let header = format_auth_results_header("mx.example.com", &[]);
        assert!(header.contains("; none"));
    }

    #[test]
    fn dmarc_temperror_result() {
        let results = vec![AuthResult {
            method: "dmarc".into(),
            result: "temperror".into(),
            reason: None,
        }];
        let header = format_auth_results("mx.example.com", &results);
        assert!(header.contains("dmarc=temperror"));
    }

    #[test]
    fn dmarc_permerror_result() {
        let results = vec![AuthResult {
            method: "dmarc".into(),
            result: "permerror".into(),
            reason: None,
        }];
        let header = format_auth_results("mx.example.com", &results);
        assert!(header.contains("dmarc=permerror"));
    }
}
