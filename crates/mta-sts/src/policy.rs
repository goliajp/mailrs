//! MTA-STS policy file parser (RFC 8461 §3.2).
//!
//! The policy body (fetched from
//! `https://mta-sts.<domain>/.well-known/mta-sts.txt`) is a sequence
//! of `key: value` lines with LF or CRLF line separators. Example:
//!
//! ```text
//! version: STSv1
//! mode: enforce
//! mx: mail.example.com
//! mx: *.example.net
//! max_age: 604800
//! ```
//!
//! Multiple `mx:` lines build up the allowed-MX list (with wildcard
//! support per §4.1). Other fields appear at most once. Lines may
//! end with LF or CRLF; trailing whitespace is trimmed.

use crate::error::MtaStsError;

/// MTA-STS enforcement mode (RFC 8461 §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyMode {
    /// `mode: enforce` — recipient mail MUST be delivered over
    /// authenticated TLS to an MX listed in `mx:`. Don't deliver if
    /// no MX matches or TLS verification fails.
    Enforce,
    /// `mode: testing` — same checks as `enforce`, but failures are
    /// only reported (via TLS-RPT or operator logs), the message is
    /// still delivered.
    Testing,
    /// `mode: none` — STS is disabled. Treat as if no policy exists.
    None,
}

impl PolicyMode {
    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "enforce" => Some(Self::Enforce),
            "testing" => Some(Self::Testing),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

/// Parsed MTA-STS policy file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// `mode:` enforcement level.
    pub mode: PolicyMode,
    /// `mx:` patterns. Each entry is either a literal hostname
    /// (`mail.example.com`) or a wildcard pattern (`*.example.com`,
    /// matching exactly one label per RFC 8461 §4.1). Order is
    /// preserved as it appears in the policy file.
    pub mx: Vec<String>,
    /// `max_age:` in seconds. Receivers cache the policy for at most
    /// this long before refreshing. RFC 8461 §3.2 says 86_400 ≤
    /// max_age ≤ 31_557_600, but we accept any non-negative integer
    /// (callers can clamp).
    pub max_age: u64,
}

impl Policy {
    /// Parse a policy file body.
    pub fn parse(body: &str) -> Result<Self, MtaStsError> {
        let mut version_seen = false;
        let mut mode: Option<PolicyMode> = None;
        let mut mx: Vec<String> = Vec::new();
        let mut max_age: Option<u64> = None;

        for line in body.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim();
            match key.as_str() {
                "version" => {
                    if !value.eq_ignore_ascii_case("STSv1") {
                        return Err(MtaStsError::UnsupportedVersion(value.to_string()));
                    }
                    version_seen = true;
                }
                "mode" => {
                    mode = Some(
                        PolicyMode::parse(value)
                            .ok_or_else(|| MtaStsError::InvalidMode(value.to_string()))?,
                    );
                }
                "mx" if !value.is_empty() => {
                    mx.push(value.to_ascii_lowercase());
                }
                "mx" => {} // empty mx: line, skip
                "max_age" => {
                    max_age = Some(
                        value
                            .parse()
                            .map_err(|_| MtaStsError::InvalidMaxAge(value.to_string()))?,
                    );
                }
                // Unknown keys ignored per RFC 8461 §3.2 (forward-compat).
                _ => {}
            }
        }

        if !version_seen {
            return Err(MtaStsError::MissingField("version"));
        }
        let mode = mode.ok_or(MtaStsError::MissingField("mode"))?;
        if mx.is_empty() {
            return Err(MtaStsError::MissingField("mx"));
        }
        let max_age = max_age.ok_or(MtaStsError::MissingField("max_age"))?;

        Ok(Self { mode, mx, max_age })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIMPLE_POLICY: &str = "\
version: STSv1
mode: enforce
mx: mail.example.com
max_age: 604800
";

    const WILDCARD_POLICY: &str = "\
version: STSv1
mode: enforce
mx: *.example.com
mx: backup.example.com
max_age: 86400
";

    #[test]
    fn parse_minimal() {
        let p = Policy::parse(SIMPLE_POLICY).unwrap();
        assert_eq!(p.mode, PolicyMode::Enforce);
        assert_eq!(p.mx, vec!["mail.example.com"]);
        assert_eq!(p.max_age, 604800);
    }

    #[test]
    fn parse_wildcard_and_multiple_mx() {
        let p = Policy::parse(WILDCARD_POLICY).unwrap();
        assert_eq!(p.mx, vec!["*.example.com", "backup.example.com"]);
    }

    #[test]
    fn parse_mode_testing() {
        let body = "version: STSv1\nmode: testing\nmx: x.example\nmax_age: 86400";
        let p = Policy::parse(body).unwrap();
        assert_eq!(p.mode, PolicyMode::Testing);
    }

    #[test]
    fn parse_mode_none() {
        let body = "version: STSv1\nmode: none\nmx: x.example\nmax_age: 86400";
        let p = Policy::parse(body).unwrap();
        assert_eq!(p.mode, PolicyMode::None);
    }

    #[test]
    fn parse_crlf_line_endings() {
        let body = "version: STSv1\r\nmode: enforce\r\nmx: m.example\r\nmax_age: 86400";
        let p = Policy::parse(body).unwrap();
        assert_eq!(p.mode, PolicyMode::Enforce);
        assert_eq!(p.mx, vec!["m.example"]);
    }

    #[test]
    fn parse_case_insensitive_field_names() {
        let body = "Version: STSv1\nMode: Enforce\nMX: m.example\nMax_Age: 86400";
        let p = Policy::parse(body).unwrap();
        assert_eq!(p.mode, PolicyMode::Enforce);
    }

    #[test]
    fn parse_skips_blank_and_comment_lines() {
        let body = "\
version: STSv1
# this is a comment
mode: enforce

mx: m.example
max_age: 86400
";
        let p = Policy::parse(body).unwrap();
        assert_eq!(p.mx, vec!["m.example"]);
    }

    #[test]
    fn parse_ignores_unknown_fields() {
        let body = "version: STSv1\nmode: enforce\nmx: m.example\nmax_age: 86400\nfuture_field: x";
        let p = Policy::parse(body).unwrap();
        assert_eq!(p.mx, vec!["m.example"]);
    }

    #[test]
    fn parse_lowercases_mx_hostnames() {
        let body = "version: STSv1\nmode: enforce\nmx: Mail.Example.COM\nmax_age: 86400";
        let p = Policy::parse(body).unwrap();
        assert_eq!(p.mx, vec!["mail.example.com"]);
    }

    #[test]
    fn parse_rejects_missing_version() {
        let body = "mode: enforce\nmx: m.example\nmax_age: 86400";
        let r = Policy::parse(body);
        assert!(matches!(r, Err(MtaStsError::MissingField("version"))));
    }

    #[test]
    fn parse_rejects_unsupported_version() {
        let body = "version: STSv2\nmode: enforce\nmx: m.example\nmax_age: 86400";
        let r = Policy::parse(body);
        assert!(matches!(r, Err(MtaStsError::UnsupportedVersion(_))));
    }

    #[test]
    fn parse_rejects_missing_mode() {
        let body = "version: STSv1\nmx: m.example\nmax_age: 86400";
        let r = Policy::parse(body);
        assert!(matches!(r, Err(MtaStsError::MissingField("mode"))));
    }

    #[test]
    fn parse_rejects_missing_mx() {
        let body = "version: STSv1\nmode: enforce\nmax_age: 86400";
        let r = Policy::parse(body);
        assert!(matches!(r, Err(MtaStsError::MissingField("mx"))));
    }

    #[test]
    fn parse_rejects_missing_max_age() {
        let body = "version: STSv1\nmode: enforce\nmx: m.example";
        let r = Policy::parse(body);
        assert!(matches!(r, Err(MtaStsError::MissingField("max_age"))));
    }

    #[test]
    fn parse_rejects_invalid_mode() {
        let body = "version: STSv1\nmode: garbage\nmx: m.example\nmax_age: 86400";
        let r = Policy::parse(body);
        assert!(matches!(r, Err(MtaStsError::InvalidMode(_))));
    }

    #[test]
    fn parse_rejects_invalid_max_age() {
        let body = "version: STSv1\nmode: enforce\nmx: m.example\nmax_age: notanumber";
        let r = Policy::parse(body);
        assert!(matches!(r, Err(MtaStsError::InvalidMaxAge(_))));
    }

    #[test]
    fn parse_empty_mx_value_skipped() {
        // Empty mx: line shouldn't count toward the required mx list.
        let body = "version: STSv1\nmode: enforce\nmx:\nmx: real.example\nmax_age: 86400";
        let p = Policy::parse(body).unwrap();
        assert_eq!(p.mx, vec!["real.example"]);
    }
}
