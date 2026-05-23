//! TLSRPT DNS TXT record parser (RFC 8460 §3).
//!
//! Format: `v=TLSRPTv1; rua=<endpoint>[,<endpoint>...]`.
//!
//! Endpoints are either `mailto:user@host` or `https://host/path`.
//! At least one is required; multiple are comma-separated. Per
//! RFC 8460 §3, reports SHOULD be delivered to all listed endpoints
//! (the receiving domain decides the actual policy).

use crate::error::TlsRptError;

/// One delivery endpoint from the `rua=` tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuaEndpoint {
    /// `mailto:user@host` — submit the report as a signed email.
    Mailto(String),
    /// `https://host/path` — POST the gzip-encoded report.
    Https(String),
}

impl RuaEndpoint {
    /// Parse one endpoint string. Returns
    /// [`TlsRptError::InvalidEndpoint`] for anything that isn't
    /// `mailto:` or `https:`. Trailing whitespace is tolerated;
    /// the URI scheme match is case-insensitive per RFC 3986 §3.1.
    pub fn parse(s: &str) -> Result<Self, TlsRptError> {
        let s = s.trim();
        let lower = s.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("mailto:") {
            let original_rest = &s[7..];
            if rest.trim().is_empty() {
                return Err(TlsRptError::InvalidEndpoint(s.to_string()));
            }
            Ok(Self::Mailto(original_rest.to_string()))
        } else if lower.starts_with("https:") {
            Ok(Self::Https(s.to_string()))
        } else {
            Err(TlsRptError::InvalidEndpoint(s.to_string()))
        }
    }
}

/// Parsed TLSRPT TXT record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsRptRecord {
    /// Delivery endpoints (`rua=` list). Non-empty by construction.
    pub rua: Vec<RuaEndpoint>,
}

impl TlsRptRecord {
    /// Parse a TLSRPT TXT record.
    ///
    /// Forward-compatible: unknown tags (e.g. proposed extensions) are
    /// silently skipped per the general DNS-TXT extensibility convention.
    /// Malformed individual rua entries cause the whole parse to fail
    /// — better to refuse the record than to silently drop an endpoint
    /// the owner intended.
    pub fn parse(txt: &str) -> Result<Self, TlsRptError> {
        let txt = txt.trim();
        // First tag MUST be v= (RFC 8460 §3 — record starts with v=).
        // We don't yet care WHAT the v= value is, only that the tag
        // is present in the lead position; otherwise this isn't a
        // TLSRPT record at all.
        let first_tag = txt.split(';').next().unwrap_or("").trim();
        let first_name = first_tag
            .split_once('=')
            .map(|(n, _)| n.trim().to_ascii_lowercase());
        if first_name.as_deref() != Some("v") {
            return Err(TlsRptError::NotATlsRptRecord);
        }
        let mut rua: Option<Vec<RuaEndpoint>> = None;
        for raw_tag in txt.split(';') {
            let tag = raw_tag.trim();
            if tag.is_empty() {
                continue;
            }
            let (name, value) = match tag.split_once('=') {
                Some((n, v)) => (n.trim().to_ascii_lowercase(), v.trim()),
                None => continue,
            };
            match name.as_str() {
                "v" if !value.eq_ignore_ascii_case("TLSRPTv1") => {
                    return Err(TlsRptError::UnsupportedVersion(value.to_string()));
                }
                "v" => {}
                "rua" => {
                    let mut out = Vec::new();
                    for part in value.split(',') {
                        if part.trim().is_empty() {
                            continue;
                        }
                        out.push(RuaEndpoint::parse(part)?);
                    }
                    if out.is_empty() {
                        return Err(TlsRptError::MissingRua);
                    }
                    rua = Some(out);
                }
                _ => {}
            }
        }
        let rua = rua.ok_or(TlsRptError::MissingRua)?;
        Ok(Self { rua })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_mailto() {
        let r = TlsRptRecord::parse("v=TLSRPTv1; rua=mailto:tlsrpt@example.com").unwrap();
        assert_eq!(r.rua.len(), 1);
        assert_eq!(
            r.rua[0],
            RuaEndpoint::Mailto("tlsrpt@example.com".into())
        );
    }

    #[test]
    fn parse_minimal_https() {
        let r =
            TlsRptRecord::parse("v=TLSRPTv1; rua=https://reports.example.com/v1/tlsrpt").unwrap();
        assert_eq!(r.rua.len(), 1);
        assert_eq!(
            r.rua[0],
            RuaEndpoint::Https("https://reports.example.com/v1/tlsrpt".into())
        );
    }

    #[test]
    fn parse_multiple_rua_endpoints() {
        let r = TlsRptRecord::parse(
            "v=TLSRPTv1; rua=mailto:tlsrpt@example.com,https://reports.example.com/tlsrpt",
        )
        .unwrap();
        assert_eq!(r.rua.len(), 2);
        assert_eq!(
            r.rua[0],
            RuaEndpoint::Mailto("tlsrpt@example.com".into())
        );
        assert_eq!(
            r.rua[1],
            RuaEndpoint::Https("https://reports.example.com/tlsrpt".into())
        );
    }

    #[test]
    fn parse_tolerates_extra_whitespace() {
        let r = TlsRptRecord::parse("  v=TLSRPTv1 ; rua=mailto:t@e.com  ").unwrap();
        assert_eq!(r.rua.len(), 1);
    }

    #[test]
    fn parse_case_insensitive_version() {
        let r = TlsRptRecord::parse("v=tlsrptv1; rua=mailto:t@e.com").unwrap();
        assert_eq!(r.rua.len(), 1);
    }

    #[test]
    fn parse_ignores_unknown_tags() {
        let r = TlsRptRecord::parse(
            "v=TLSRPTv1; rua=mailto:t@e.com; future=whatever; other=42",
        )
        .unwrap();
        assert_eq!(r.rua.len(), 1);
    }

    #[test]
    fn parse_rejects_missing_v() {
        let r = TlsRptRecord::parse("rua=mailto:t@e.com");
        assert!(matches!(r, Err(TlsRptError::NotATlsRptRecord)));
    }

    #[test]
    fn parse_rejects_wrong_version() {
        let r = TlsRptRecord::parse("v=TLSRPTv2; rua=mailto:t@e.com");
        assert!(matches!(r, Err(TlsRptError::UnsupportedVersion(_))));
    }

    #[test]
    fn parse_rejects_missing_rua() {
        let r = TlsRptRecord::parse("v=TLSRPTv1");
        assert!(matches!(r, Err(TlsRptError::MissingRua)));
    }

    #[test]
    fn parse_rejects_empty_rua() {
        let r = TlsRptRecord::parse("v=TLSRPTv1; rua=");
        // Empty `rua=` (no endpoints) is the same as missing — surface
        // it as MissingRua so the caller sees one error class.
        assert!(matches!(r, Err(TlsRptError::MissingRua)));
    }

    #[test]
    fn parse_rejects_unknown_scheme() {
        let r = TlsRptRecord::parse("v=TLSRPTv1; rua=ftp://reports.example.com/");
        assert!(matches!(r, Err(TlsRptError::InvalidEndpoint(_))));
    }

    #[test]
    fn parse_rejects_mailto_empty_address() {
        let r = TlsRptRecord::parse("v=TLSRPTv1; rua=mailto:");
        assert!(matches!(r, Err(TlsRptError::InvalidEndpoint(_))));
    }

    #[test]
    fn endpoint_parse_https_case_insensitive() {
        let e = RuaEndpoint::parse("HTTPS://Reports.example.com/").unwrap();
        assert!(matches!(e, RuaEndpoint::Https(_)));
    }

    #[test]
    fn endpoint_parse_mailto_preserves_case_in_localpart() {
        // mailto: scheme is case-insensitive but the address itself
        // must be preserved verbatim (RFC 6068 §2 — local-part is
        // case-sensitive at the destination).
        let e = RuaEndpoint::parse("mailto:TLSrpt@Example.COM").unwrap();
        assert_eq!(e, RuaEndpoint::Mailto("TLSrpt@Example.COM".into()));
    }
}
