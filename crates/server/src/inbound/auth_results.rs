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
}
