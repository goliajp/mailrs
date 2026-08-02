//! Parsing one mechanism out of a record.

//! SPF record parsing (RFC 7208 §4.6).
//!
//! Turns the raw TXT string (`"v=spf1 ip4:1.2.3.4 include:example.com -all"`)
//! into a typed [`Record`] with [`Mechanism`]s + [`Qualifier`]s.

use std::net::Ipv6Addr;

use compact_str::CompactString;

use super::*;
use crate::error::SpfError;

#[inline]
pub(crate) fn parse_mechanism(token: &str) -> Result<Mechanism, SpfError> {
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

    // Byte-match on the mechanism name. Avoids the UTF-8-aware `&str` match
    // path — mechanism names are pure ASCII so the byte form is strictly
    // cheaper at runtime.
    match name.as_bytes() {
        b"all" => {
            if value.is_some() {
                return Err(SpfError::InvalidRecord(format!(
                    "'all' takes no value: {token}"
                )));
            }
            Ok(Mechanism::All { qualifier })
        }
        b"ip4" => {
            let v = value.ok_or_else(|| SpfError::InvalidRecord("ip4: missing value".into()))?;
            let (addr_str, prefix) = parse_addr_and_prefix(v, 32)?;
            // Hand-rolled byte-level IPv4 parser. `std::net::Ipv4Addr::FromStr`
            // pays for general-purpose error reporting + UTF-8 char iter.
            // For dotted-quad ASCII input we can do it in ~5 ns by walking
            // bytes once and rejecting on the first non-digit/non-dot. This
            // closes the +25% gap vs `mail-auth` 0.9 on simple records.
            let addr = parse_ipv4_fast(addr_str)
                .ok_or_else(|| SpfError::InvalidRecord(format!("bad ipv4 address: {addr_str}")))?;
            Ok(Mechanism::Ip4 {
                qualifier,
                addr,
                prefix,
            })
        }
        b"ip6" => {
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
        b"a" => {
            let (domain, ip4_prefix, ip6_prefix) = parse_a_mx_value(value)?;
            Ok(Mechanism::A {
                qualifier,
                domain,
                ip4_prefix,
                ip6_prefix,
            })
        }
        b"mx" => {
            let (domain, ip4_prefix, ip6_prefix) = parse_a_mx_value(value)?;
            Ok(Mechanism::Mx {
                qualifier,
                domain,
                ip4_prefix,
                ip6_prefix,
            })
        }
        b"include" => {
            let v =
                value.ok_or_else(|| SpfError::InvalidRecord("include: missing domain".into()))?;
            Ok(Mechanism::Include {
                qualifier,
                domain: CompactString::new(v),
            })
        }
        b"exists" => {
            let v =
                value.ok_or_else(|| SpfError::InvalidRecord("exists: missing domain".into()))?;
            Ok(Mechanism::Exists {
                qualifier,
                domain: CompactString::new(v),
            })
        }
        b"ptr" => {
            // RFC 7208 §5.5 marks ptr as not-recommended; v1.0 of this
            // crate doesn't implement PTR lookups → permerror.
            Err(SpfError::InvalidRecord(
                "ptr mechanism not supported (RFC 7208 §5.5 deprecates)".into(),
            ))
        }
        _ => Err(SpfError::InvalidRecord(format!(
            "unknown mechanism: {name}"
        ))),
    }
}

#[inline]
pub(crate) fn split_qualifier(token: &str) -> (Qualifier, &str) {
    if let Some(first) = token.as_bytes().first()
        && let Some(q) = Qualifier::from_byte(*first)
    {
        return (q, &token[1..]);
    }
    (Qualifier::Pass, token) // default qualifier is `+`
}

/// Parse the optional `:domain/prefix4//prefix6` suffix on `a` and `mx`.
pub(crate) fn parse_a_mx_value(
    value: Option<&str>,
) -> Result<(Option<CompactString>, u8, u8), SpfError> {
    let Some(v) = value else {
        return Ok((None, 32, 128));
    };
    let (domain_part, prefix_part) = match v.find('/') {
        Some(idx) => (Some(&v[..idx]), &v[idx..]),
        None => (Some(v), ""),
    };
    let domain = domain_part
        .filter(|s| !s.is_empty())
        .map(CompactString::new);

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

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
        if let Mechanism::A {
            domain,
            ip4_prefix,
            ip6_prefix,
            ..
        } = &r.mechanisms[0]
        {
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
        if let Mechanism::A {
            ip4_prefix,
            ip6_prefix,
            ..
        } = r.mechanisms[0]
        {
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
