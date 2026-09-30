//! Macro expansion for domain-specs (RFC 7208 §7).
//!
//! `include:%{ir}.%{v}.%{d}.spf.example.net` is expanded against the
//! connecting IP, the sender and the domain currently being evaluated
//! before any DNS lookup is made.

use std::borrow::Cow;
use std::net::IpAddr;

use crate::error::SpfError;
use crate::evaluator::VerifyInput;

/// Longest name a lookup may use (RFC 7208 §7.3).
const MAX_DOMAIN_LEN: usize = 253;

/// Expand every macro in `spec`. Specs without `%` are returned as-is.
///
/// `current_domain` is the domain whose record is being evaluated (`%{d}`).
///
/// ```
/// use mailrs_spf::{VerifyInput, macros::expand};
/// let input = VerifyInput {
///     ip: "192.0.2.3".parse().unwrap(),
///     helo: "mx.example.com".into(),
///     mail_from: "strong-bad@email.example.com".into(),
/// };
/// let name = expand("%{ir}.%{v}._spf.%{d2}", &input, "email.example.com").unwrap();
/// assert_eq!(name, "3.2.0.192.in-addr._spf.example.com");
/// ```
pub fn expand<'a>(
    spec: &'a str,
    input: &VerifyInput,
    current_domain: &str,
) -> Result<Cow<'a, str>, SpfError> {
    if !spec.contains('%') {
        return Ok(Cow::Borrowed(spec));
    }
    let mut out = String::with_capacity(spec.len() + 32);
    let mut rest = spec;
    while let Some(pos) = rest.find('%') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        match after.as_bytes().first() {
            Some(b'%') => {
                out.push('%');
                rest = &after[1..];
            }
            Some(b'_') => {
                out.push(' ');
                rest = &after[1..];
            }
            Some(b'-') => {
                out.push_str("%20");
                rest = &after[1..];
            }
            Some(b'{') => {
                let end = after
                    .find('}')
                    .ok_or_else(|| invalid(spec, "unterminated macro"))?;
                expand_one(&after[1..end], input, current_domain, spec, &mut out)?;
                rest = &after[end + 1..];
            }
            _ => return Err(invalid(spec, "stray '%'")),
        }
    }
    out.push_str(rest);
    Ok(Cow::Owned(truncate_left(out)))
}

/// One `%{...}` body: letter, optional digits, optional `r`, delimiters.
fn expand_one(
    body: &str,
    input: &VerifyInput,
    current_domain: &str,
    spec: &str,
    out: &mut String,
) -> Result<(), SpfError> {
    let mut chars = body.char_indices();
    let (_, letter) = chars.next().ok_or_else(|| invalid(spec, "empty macro"))?;
    let value = macro_value(letter.to_ascii_lowercase(), input, current_domain)
        .ok_or_else(|| invalid(spec, "unknown macro letter"))?;

    let tail = &body[letter.len_utf8()..];
    let digits_end = tail.bytes().take_while(u8::is_ascii_digit).count();
    let keep = match digits_end {
        0 => None,
        _ => match tail[..digits_end].parse::<usize>() {
            Ok(0) | Err(_) => return Err(invalid(spec, "bad label count")),
            Ok(n) => Some(n),
        },
    };
    let mut tail = &tail[digits_end..];
    let reverse = tail.starts_with(['r', 'R']);
    if reverse {
        tail = &tail[1..];
    }
    if !tail
        .bytes()
        .all(|b| matches!(b, b'.' | b'-' | b'+' | b',' | b'/' | b'_' | b'='))
    {
        return Err(invalid(spec, "bad delimiter"));
    }
    let delims = match tail {
        "" => ".",
        d => d,
    };

    let mut labels: Vec<&str> = value.split(|c| delims.contains(c)).collect();
    if reverse {
        labels.reverse();
    }
    if let Some(n) = keep {
        labels.drain(..labels.len().saturating_sub(n));
    }
    let joined = labels.join(".");
    if letter.is_ascii_uppercase() {
        url_escape(&joined, out);
    } else {
        out.push_str(&joined);
    }
    Ok(())
}

fn macro_value(letter: char, input: &VerifyInput, current_domain: &str) -> Option<String> {
    let (local, sender_domain) = match input.mail_from.rsplit_once('@') {
        Some((l, d)) if !l.is_empty() => (l, d),
        Some((_, d)) => ("postmaster", d),
        None => ("postmaster", input.target_domain()),
    };
    match letter {
        's' => Some(format!("{local}@{sender_domain}")),
        'l' => Some(local.to_string()),
        'o' => Some(sender_domain.to_string()),
        'd' => Some(current_domain.to_string()),
        'i' => Some(dotted_ip(input.ip)),
        // validating the client's reverse name costs extra lookups and
        // the RFC discourages the macro; "unknown" is its defined fallback
        'p' => Some("unknown".to_string()),
        'v' => Some(match input.ip {
            IpAddr::V4(_) => "in-addr".to_string(),
            IpAddr::V6(_) => "ip6".to_string(),
        }),
        'h' => Some(input.helo.clone()),
        _ => None,
    }
}

/// `%{i}`: dotted quad for IPv4, dot-separated nibbles for IPv6.
fn dotted_ip(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => {
            let mut s = String::with_capacity(63);
            for byte in v6.octets() {
                for nibble in [byte >> 4, byte & 0xf] {
                    if !s.is_empty() {
                        s.push('.');
                    }
                    s.push(char::from(b"0123456789abcdef"[usize::from(nibble)]));
                }
            }
            s
        }
    }
}

/// Uppercase macro letters URL-escape everything outside RFC 3986 "unreserved".
fn url_escape(s: &str, out: &mut String) {
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
}

/// Over-long names lose labels from the left until they fit (RFC 7208 §7.3).
fn truncate_left(mut name: String) -> String {
    while name.len() > MAX_DOMAIN_LEN {
        match name.find('.') {
            Some(dot) => {
                name.drain(..=dot);
            }
            None => break,
        }
    }
    name
}

fn invalid(spec: &str, why: &str) -> SpfError {
    SpfError::InvalidRecord(format!("{why} in macro: {spec}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // examples from RFC 7208 §7.4
    fn rfc_input(ip: &str) -> VerifyInput {
        VerifyInput {
            ip: ip.parse().unwrap(),
            helo: "mx.example.org".into(),
            mail_from: "strong-bad@email.example.com".into(),
        }
    }

    fn x(spec: &str) -> String {
        expand(spec, &rfc_input("192.0.2.3"), "email.example.com")
            .unwrap()
            .into_owned()
    }

    #[test]
    fn rfc_examples_ipv4() {
        assert_eq!(x("%{s}"), "strong-bad@email.example.com");
        assert_eq!(x("%{o}"), "email.example.com");
        assert_eq!(x("%{d}"), "email.example.com");
        assert_eq!(x("%{d4}"), "email.example.com");
        assert_eq!(x("%{d3}"), "email.example.com");
        assert_eq!(x("%{d2}"), "example.com");
        assert_eq!(x("%{d1}"), "com");
        assert_eq!(x("%{dr}"), "com.example.email");
        assert_eq!(x("%{d2r}"), "example.email");
        assert_eq!(x("%{l}"), "strong-bad");
        assert_eq!(x("%{l-}"), "strong.bad");
        assert_eq!(x("%{lr}"), "strong-bad");
        assert_eq!(x("%{lr-}"), "bad.strong");
        assert_eq!(x("%{l1r-}"), "strong");
        assert_eq!(
            x("%{ir}.%{v}._spf.%{d2}"),
            "3.2.0.192.in-addr._spf.example.com"
        );
        assert_eq!(x("%{lr-}.lp._spf.%{d2}"), "bad.strong.lp._spf.example.com");
        assert_eq!(
            x("%{lr-}.lp.%{ir}.%{v}._spf.%{d2}"),
            "bad.strong.lp.3.2.0.192.in-addr._spf.example.com"
        );
        assert_eq!(
            x("%{ir}.%{v}.%{l1r-}.lp._spf.%{d2}"),
            "3.2.0.192.in-addr.strong.lp._spf.example.com"
        );
        assert_eq!(
            x("%{d2}.trusted-domains.example.net"),
            "example.com.trusted-domains.example.net"
        );
    }

    #[test]
    fn rfc_example_ipv6() {
        let input = rfc_input("2001:db8::cb01");
        let got = expand("%{ir}.%{v}._spf.%{d2}", &input, "email.example.com").unwrap();
        assert_eq!(
            got,
            "1.0.b.c.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.8.b.d.0.1.0.0.2.ip6._spf.example.com"
        );
    }

    #[test]
    fn hosted_spf_include() {
        let input = VerifyInput {
            ip: "74.125.227.141".parse().unwrap(),
            helo: "mail-oa1-f13.google.com".into(),
            mail_from: "no-reply@focusai.com".into(),
        };
        let got = expand(
            "%{ir}.%{v}.%{d}.spf.has.pphosted.com",
            &input,
            "focusai.com",
        )
        .unwrap();
        assert_eq!(
            got,
            "141.227.125.74.in-addr.focusai.com.spf.has.pphosted.com"
        );
    }

    #[test]
    fn escapes_and_url_encoding() {
        assert_eq!(x("a%%b%_c%-d"), "a%b c%20d");
        assert_eq!(x("%{L}"), "strong-bad");
        assert_eq!(x("%{S}"), "strong-bad%40email.example.com");
    }

    #[test]
    fn empty_local_part_is_postmaster() {
        let mut input = rfc_input("192.0.2.3");
        input.mail_from = "@email.example.com".into();
        assert_eq!(expand("%{l}", &input, "d").unwrap(), "postmaster");
        input.mail_from = String::new();
        assert_eq!(
            expand("%{s}", &input, "d").unwrap(),
            "postmaster@mx.example.org"
        );
    }

    #[test]
    fn literal_spec_is_borrowed() {
        assert!(matches!(
            expand("_spf.example.com", &rfc_input("192.0.2.3"), "d").unwrap(),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn malformed_macros_are_permerror() {
        let input = rfc_input("192.0.2.3");
        for bad in ["%{", "%{}", "%{q}", "%{d0}", "%{d:}", "50%", "%x"] {
            assert!(
                matches!(expand(bad, &input, "d"), Err(SpfError::InvalidRecord(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn long_names_drop_left_labels() {
        let mut input = rfc_input("192.0.2.3");
        input.mail_from = format!("{}@example.com", "a.".repeat(150) + "b");
        let got = expand("%{l}.example.com", &input, "d").unwrap();
        assert!(got.len() <= MAX_DOMAIN_LEN);
        assert!(got.ends_with(".b.example.com"));
    }
}
