//! SPF record parsing (RFC 7208 §4.6).
//!
//! Turns the raw TXT string (`"v=spf1 ip4:1.2.3.4 include:example.com -all"`)
//! into a typed [`Record`] with [`Mechanism`]s + [`Qualifier`]s.

use crate::error::SpfError;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// SPF qualifier (RFC 7208 §4.6.2). Default is `Pass` (`+`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Qualifier {
    /// `+` — Pass on match.
    Pass,
    /// `-` — Fail on match.
    Fail,
    /// `~` — SoftFail on match.
    SoftFail,
    /// `?` — Neutral on match.
    Neutral,
}

impl Qualifier {
    fn from_byte(b: u8) -> Option<Self> {
        match b {
            b'+' => Some(Qualifier::Pass),
            b'-' => Some(Qualifier::Fail),
            b'~' => Some(Qualifier::SoftFail),
            b'?' => Some(Qualifier::Neutral),
            _ => None,
        }
    }
}

/// One SPF mechanism (RFC 7208 §5).
///
/// Each mechanism carries its [`Qualifier`] and the mechanism-specific
/// payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mechanism {
    /// `all` — matches every IP.
    All {
        /// Qualifier applied on match.
        qualifier: Qualifier,
    },
    /// `ip4:1.2.3.4` or `ip4:1.2.3.0/24` — matches IPv4 in the
    /// specified network.
    Ip4 {
        /// Qualifier applied on match.
        qualifier: Qualifier,
        /// Network base address.
        addr: Ipv4Addr,
        /// Prefix length (1-32). 32 if not specified in the record.
        prefix: u8,
    },
    /// `ip6:2001:db8::1` or `ip6:2001:db8::/32` — matches IPv6.
    Ip6 {
        /// Qualifier applied on match.
        qualifier: Qualifier,
        /// Network base address.
        addr: Ipv6Addr,
        /// Prefix length (1-128). 128 if not specified in the record.
        prefix: u8,
    },
    /// `a` or `a:example.com` or `a:example.com/24`.
    A {
        /// Qualifier applied on match.
        qualifier: Qualifier,
        /// Domain to look up (default = current domain in scope).
        domain: Option<String>,
        /// IPv4 prefix length (default 32).
        ip4_prefix: u8,
        /// IPv6 prefix length (default 128).
        ip6_prefix: u8,
    },
    /// `mx` or `mx:example.com`.
    Mx {
        /// Qualifier applied on match.
        qualifier: Qualifier,
        /// Domain whose MX records to look up.
        domain: Option<String>,
        /// IPv4 prefix length (default 32).
        ip4_prefix: u8,
        /// IPv6 prefix length (default 128).
        ip6_prefix: u8,
    },
    /// `include:example.com` — recurse into another domain's SPF.
    Include {
        /// Qualifier applied on match.
        qualifier: Qualifier,
        /// Included domain to resolve recursively.
        domain: String,
    },
    /// `exists:%{ir}.example.com` — match if the lookup returns ANY A.
    Exists {
        /// Qualifier applied on match.
        qualifier: Qualifier,
        /// Domain template to look up. Macro expansion is out of v1.0
        /// scope; the literal template is used as-is.
        domain: String,
    },
}

impl Mechanism {
    /// Qualifier accessor (every variant has one).
    pub fn qualifier(&self) -> Qualifier {
        match self {
            Mechanism::All { qualifier }
            | Mechanism::Ip4 { qualifier, .. }
            | Mechanism::Ip6 { qualifier, .. }
            | Mechanism::A { qualifier, .. }
            | Mechanism::Mx { qualifier, .. }
            | Mechanism::Include { qualifier, .. }
            | Mechanism::Exists { qualifier, .. } => *qualifier,
        }
    }
}

/// Parsed SPF record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// All mechanisms in document order (the evaluator walks them
    /// left-to-right and stops at the first non-implicit match).
    pub mechanisms: Vec<Mechanism>,
}

impl Record {
    /// Parse a TXT-record string as an SPF record.
    ///
    /// Returns `SpfError::InvalidRecord` if the input doesn't start
    /// with `v=spf1` or contains an unparseable mechanism.
    ///
    /// ```
    /// use mailrs_spf::Record;
    /// let r = Record::parse("v=spf1 ip4:203.0.113.0/24 include:example.com -all").unwrap();
    /// assert_eq!(r.mechanisms.len(), 3);
    /// ```
    pub fn parse(input: &str) -> Result<Self, SpfError> {
        let trimmed = input.trim();
        let after_version = trimmed
            .strip_prefix("v=spf1")
            .ok_or_else(|| SpfError::InvalidRecord("missing v=spf1 prefix".into()))?;
        // After `v=spf1`, mechanisms are space-separated. Empty record
        // (just `v=spf1`) is valid → no mechanisms → defaults to None.
        let mut mechanisms = Vec::new();
        for token in after_version.split_whitespace() {
            // Skip modifiers (`name=value`) for the v1.0 surface —
            // `redirect=` and `exp=` aren't evaluated yet (out of scope
            // per CHANGELOG; PR welcome).
            if token.contains('=') && !Self::is_mechanism_with_value(token) {
                continue;
            }
            mechanisms.push(parse_mechanism(token)?);
        }
        Ok(Record { mechanisms })
    }

    /// Distinguish `ip4:1.2.3.4` (mechanism with value, uses `:`) from
    /// `redirect=example.com` (modifier, uses `=`).
    fn is_mechanism_with_value(token: &str) -> bool {
        // Mechanisms use `:` for their value separator, modifiers use `=`.
        // If `=` appears before any `:`, it's a modifier.
        match (token.find('='), token.find(':')) {
            (Some(eq), Some(colon)) => colon < eq,
            (Some(_), None) => false,
            _ => true,
        }
    }
}

fn parse_mechanism(token: &str) -> Result<Mechanism, SpfError> {
    let (qualifier, body) = split_qualifier(token);

    // Split mechanism name from value
    let (name, value) = match body.split_once(':') {
        Some((n, v)) => (n, Some(v)),
        None => {
            // Could be `a` or `a/24` (prefix without explicit domain)
            if let Some((n, _)) = body.split_once('/') {
                (n, Some(&body[n.len()..])) // include the '/' in value
            } else {
                (body, None)
            }
        }
    };

    match name {
        "all" => {
            if value.is_some() {
                return Err(SpfError::InvalidRecord(format!(
                    "'all' takes no value: {token}"
                )));
            }
            Ok(Mechanism::All { qualifier })
        }
        "ip4" => {
            let v = value.ok_or_else(|| SpfError::InvalidRecord("ip4: missing value".into()))?;
            let (addr_str, prefix) = parse_addr_and_prefix(v, 32)?;
            let addr: Ipv4Addr = addr_str
                .parse()
                .map_err(|_| SpfError::InvalidRecord(format!("bad ipv4 address: {addr_str}")))?;
            Ok(Mechanism::Ip4 {
                qualifier,
                addr,
                prefix,
            })
        }
        "ip6" => {
            let v = value.ok_or_else(|| SpfError::InvalidRecord("ip6: missing value".into()))?;
            let (addr_str, prefix) = parse_addr_and_prefix(v, 128)?;
            let addr: Ipv6Addr = addr_str
                .parse()
                .map_err(|_| SpfError::InvalidRecord(format!("bad ipv6 address: {addr_str}")))?;
            Ok(Mechanism::Ip6 {
                qualifier,
                addr,
                prefix,
            })
        }
        "a" => {
            let (domain, ip4_prefix, ip6_prefix) = parse_a_mx_value(value)?;
            Ok(Mechanism::A {
                qualifier,
                domain,
                ip4_prefix,
                ip6_prefix,
            })
        }
        "mx" => {
            let (domain, ip4_prefix, ip6_prefix) = parse_a_mx_value(value)?;
            Ok(Mechanism::Mx {
                qualifier,
                domain,
                ip4_prefix,
                ip6_prefix,
            })
        }
        "include" => {
            let v = value
                .ok_or_else(|| SpfError::InvalidRecord("include: missing domain".into()))?;
            Ok(Mechanism::Include {
                qualifier,
                domain: v.to_string(),
            })
        }
        "exists" => {
            let v = value
                .ok_or_else(|| SpfError::InvalidRecord("exists: missing domain".into()))?;
            Ok(Mechanism::Exists {
                qualifier,
                domain: v.to_string(),
            })
        }
        "ptr" => {
            // RFC 7208 §5.5 marks ptr as not-recommended; we treat it
            // as `+all`-equivalent (always-match) when the qualifier
            // is `+`, otherwise we follow the qualifier. Out of v1.0
            // scope to actually do PTR lookups — return permerror.
            Err(SpfError::InvalidRecord(
                "ptr mechanism not supported (RFC 7208 §5.5 deprecates)".into(),
            ))
        }
        other => Err(SpfError::InvalidRecord(format!(
            "unknown mechanism: {other}"
        ))),
    }
}

fn split_qualifier(token: &str) -> (Qualifier, &str) {
    if let Some(first) = token.as_bytes().first()
        && let Some(q) = Qualifier::from_byte(*first)
    {
        return (q, &token[1..]);
    }
    (Qualifier::Pass, token) // default qualifier is `+`
}

fn parse_addr_and_prefix(value: &str, default: u8) -> Result<(String, u8), SpfError> {
    if let Some((addr, prefix_str)) = value.rsplit_once('/') {
        let prefix: u8 = prefix_str
            .parse()
            .map_err(|_| SpfError::InvalidRecord(format!("bad prefix: {prefix_str}")))?;
        Ok((addr.to_string(), prefix))
    } else {
        Ok((value.to_string(), default))
    }
}

/// Parse the optional `:domain/prefix4//prefix6` suffix on `a` and `mx`.
fn parse_a_mx_value(
    value: Option<&str>,
) -> Result<(Option<String>, u8, u8), SpfError> {
    let Some(v) = value else {
        return Ok((None, 32, 128));
    };
    // Form: [domain][/ip4_prefix][//ip6_prefix]
    let (domain_part, prefix_part) = match v.find('/') {
        Some(idx) => (Some(&v[..idx]), &v[idx..]),
        None => (Some(v), ""),
    };
    let domain = domain_part
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let (ip4_prefix, ip6_prefix) = if prefix_part.is_empty() {
        (32u8, 128u8)
    } else if let Some(rest) = prefix_part.strip_prefix("//") {
        // only //ip6
        let p6: u8 = rest
            .parse()
            .map_err(|_| SpfError::InvalidRecord(format!("bad ip6 prefix: {rest}")))?;
        (32, p6)
    } else if let Some(rest) = prefix_part.strip_prefix('/') {
        if let Some((p4_str, p6_str)) = rest.split_once("//") {
            let p4: u8 = p4_str
                .parse()
                .map_err(|_| SpfError::InvalidRecord(format!("bad ip4 prefix: {p4_str}")))?;
            let p6: u8 = p6_str
                .parse()
                .map_err(|_| SpfError::InvalidRecord(format!("bad ip6 prefix: {p6_str}")))?;
            (p4, p6)
        } else {
            let p4: u8 = rest
                .parse()
                .map_err(|_| SpfError::InvalidRecord(format!("bad ip4 prefix: {rest}")))?;
            (p4, 128)
        }
    } else {
        (32, 128)
    };

    Ok((domain, ip4_prefix, ip6_prefix))
}

/// Check whether `ip` falls in `subnet/prefix`.
pub(crate) fn ip_in_subnet(ip: IpAddr, subnet: IpAddr, prefix: u8) -> bool {
    match (ip, subnet) {
        (IpAddr::V4(a), IpAddr::V4(b)) => {
            if prefix == 0 {
                return true;
            }
            if prefix > 32 {
                return false;
            }
            let mask: u32 = if prefix == 32 { u32::MAX } else { !((1u32 << (32 - prefix)) - 1) };
            (u32::from_be_bytes(a.octets()) & mask) == (u32::from_be_bytes(b.octets()) & mask)
        }
        (IpAddr::V6(a), IpAddr::V6(b)) => {
            if prefix == 0 {
                return true;
            }
            if prefix > 128 {
                return false;
            }
            let a_bits = u128::from_be_bytes(a.octets());
            let b_bits = u128::from_be_bytes(b.octets());
            let mask: u128 = if prefix == 128 {
                u128::MAX
            } else {
                !((1u128 << (128 - prefix)) - 1)
            };
            (a_bits & mask) == (b_bits & mask)
        }
        // Mixed v4/v6 — never match.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_all_record() {
        let r = Record::parse("v=spf1 -all").unwrap();
        assert_eq!(r.mechanisms.len(), 1);
        assert_eq!(
            r.mechanisms[0],
            Mechanism::All {
                qualifier: Qualifier::Fail
            }
        );
    }

    #[test]
    fn parse_record_with_ip4() {
        let r = Record::parse("v=spf1 ip4:203.0.113.0/24 -all").unwrap();
        assert_eq!(r.mechanisms.len(), 2);
        assert_eq!(
            r.mechanisms[0],
            Mechanism::Ip4 {
                qualifier: Qualifier::Pass,
                addr: "203.0.113.0".parse().unwrap(),
                prefix: 24,
            }
        );
    }

    #[test]
    fn parse_record_with_ip4_no_prefix() {
        let r = Record::parse("v=spf1 ip4:1.2.3.4 -all").unwrap();
        if let Mechanism::Ip4 { prefix, .. } = r.mechanisms[0] {
            assert_eq!(prefix, 32);
        } else {
            panic!("expected ip4");
        }
    }

    #[test]
    fn parse_record_with_ip6() {
        let r = Record::parse("v=spf1 ip6:2001:db8::/32 -all").unwrap();
        assert_eq!(
            r.mechanisms[0],
            Mechanism::Ip6 {
                qualifier: Qualifier::Pass,
                addr: "2001:db8::".parse().unwrap(),
                prefix: 32,
            }
        );
    }

    #[test]
    fn parse_record_with_include() {
        let r = Record::parse("v=spf1 include:_spf.google.com -all").unwrap();
        assert_eq!(
            r.mechanisms[0],
            Mechanism::Include {
                qualifier: Qualifier::Pass,
                domain: "_spf.google.com".into(),
            }
        );
    }

    #[test]
    fn parse_record_with_softfail_all() {
        let r = Record::parse("v=spf1 ~all").unwrap();
        assert_eq!(
            r.mechanisms[0],
            Mechanism::All {
                qualifier: Qualifier::SoftFail
            }
        );
    }

    #[test]
    fn parse_record_with_neutral_all() {
        let r = Record::parse("v=spf1 ?all").unwrap();
        assert_eq!(
            r.mechanisms[0],
            Mechanism::All {
                qualifier: Qualifier::Neutral
            }
        );
    }

    #[test]
    fn parse_record_with_a_default() {
        let r = Record::parse("v=spf1 a -all").unwrap();
        assert_eq!(
            r.mechanisms[0],
            Mechanism::A {
                qualifier: Qualifier::Pass,
                domain: None,
                ip4_prefix: 32,
                ip6_prefix: 128,
            }
        );
    }

    #[test]
    fn parse_record_with_a_explicit_domain() {
        let r = Record::parse("v=spf1 a:example.com -all").unwrap();
        assert_eq!(
            r.mechanisms[0],
            Mechanism::A {
                qualifier: Qualifier::Pass,
                domain: Some("example.com".into()),
                ip4_prefix: 32,
                ip6_prefix: 128,
            }
        );
    }

    #[test]
    fn parse_record_with_a_and_prefix() {
        let r = Record::parse("v=spf1 a:example.com/24 -all").unwrap();
        if let Mechanism::A { domain, ip4_prefix, ip6_prefix, .. } = &r.mechanisms[0] {
            assert_eq!(domain.as_deref(), Some("example.com"));
            assert_eq!(*ip4_prefix, 24);
            assert_eq!(*ip6_prefix, 128);
        } else {
            panic!("expected a");
        }
    }

    #[test]
    fn parse_record_with_a_v4_and_v6_prefixes() {
        let r = Record::parse("v=spf1 a:example.com/24//64 -all").unwrap();
        if let Mechanism::A { ip4_prefix, ip6_prefix, .. } = r.mechanisms[0] {
            assert_eq!(ip4_prefix, 24);
            assert_eq!(ip6_prefix, 64);
        } else {
            panic!("expected a");
        }
    }

    #[test]
    fn parse_record_with_mx() {
        let r = Record::parse("v=spf1 mx -all").unwrap();
        assert!(matches!(r.mechanisms[0], Mechanism::Mx { .. }));
    }

    #[test]
    fn parse_record_with_exists() {
        let r = Record::parse("v=spf1 exists:%{i}._spf.example.com -all").unwrap();
        if let Mechanism::Exists { domain, .. } = &r.mechanisms[0] {
            assert_eq!(domain, "%{i}._spf.example.com");
        } else {
            panic!("expected exists");
        }
    }

    #[test]
    fn parse_record_rejects_missing_version() {
        let r = Record::parse("ip4:1.2.3.4 -all");
        assert!(matches!(r, Err(SpfError::InvalidRecord(_))));
    }

    #[test]
    fn parse_record_rejects_unknown_mechanism() {
        let r = Record::parse("v=spf1 frobnicate -all");
        assert!(matches!(r, Err(SpfError::InvalidRecord(_))));
    }

    #[test]
    fn parse_record_rejects_ptr_mechanism() {
        let r = Record::parse("v=spf1 ptr -all");
        assert!(matches!(r, Err(SpfError::InvalidRecord(_))));
    }

    #[test]
    fn parse_record_skips_modifiers() {
        // `redirect=` is a modifier, not a mechanism — silently skipped in v1.0.
        let r = Record::parse("v=spf1 redirect=spf.example.com").unwrap();
        assert_eq!(r.mechanisms.len(), 0);
    }

    #[test]
    fn parse_empty_record_after_version() {
        let r = Record::parse("v=spf1").unwrap();
        assert_eq!(r.mechanisms.len(), 0);
    }

    #[test]
    fn parse_record_handles_extra_whitespace() {
        let r = Record::parse("  v=spf1   ip4:1.2.3.4   -all  ").unwrap();
        assert_eq!(r.mechanisms.len(), 2);
    }

    #[test]
    fn ip_in_subnet_ipv4_exact_match() {
        let ip: IpAddr = "203.0.113.42".parse().unwrap();
        let net: IpAddr = "203.0.113.0".parse().unwrap();
        assert!(ip_in_subnet(ip, net, 24));
        assert!(!ip_in_subnet(ip, net, 32));
    }

    #[test]
    fn ip_in_subnet_ipv4_zero_prefix() {
        let ip: IpAddr = "1.2.3.4".parse().unwrap();
        let net: IpAddr = "0.0.0.0".parse().unwrap();
        assert!(ip_in_subnet(ip, net, 0));
    }

    #[test]
    fn ip_in_subnet_ipv6_match() {
        let ip: IpAddr = "2001:db8::1".parse().unwrap();
        let net: IpAddr = "2001:db8::".parse().unwrap();
        // /32 matches: prefix covers the first 32 bits which agree
        assert!(ip_in_subnet(ip, net, 32));
        // /128 should NOT match because the host bits differ
        assert!(!ip_in_subnet(ip, net, 128));
        // But /127 should match because last bit is masked off
        assert!(ip_in_subnet(ip, net, 127));
    }

    #[test]
    fn ip_in_subnet_v4_v6_mixed_never_matches() {
        let v4: IpAddr = "1.2.3.4".parse().unwrap();
        let v6: IpAddr = "2001:db8::1".parse().unwrap();
        assert!(!ip_in_subnet(v4, v6, 0));
        assert!(!ip_in_subnet(v6, v4, 0));
    }
}
