//! SPF record parsing (RFC 7208 §4.6).
//!
//! Turns the raw TXT string (`"v=spf1 ip4:1.2.3.4 include:example.com -all"`)
//! into a typed [`Record`] with [`Mechanism`]s + [`Qualifier`]s.

use std::net::{Ipv4Addr, Ipv6Addr};

use compact_str::CompactString;

use crate::error::SpfError;

mod ip;
mod parse;

pub(crate) use ip::*;
pub(crate) use parse::*;

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
        ///
        /// **v2 change**: `CompactString` (inlined ≤24 bytes); real SPF
        /// domains nearly always fit.
        domain: Option<CompactString>,
        /// IPv4 prefix length (default 32).
        ip4_prefix: u8,
        /// IPv6 prefix length (default 128).
        ip6_prefix: u8,
    },
    /// `mx` or `mx:example.com`.
    Mx {
        /// Qualifier applied on match.
        qualifier: Qualifier,
        /// Domain whose MX records to look up. `CompactString` per `A.domain`.
        domain: Option<CompactString>,
        /// IPv4 prefix length (default 32).
        ip4_prefix: u8,
        /// IPv6 prefix length (default 128).
        ip6_prefix: u8,
    },
    /// `include:example.com` — recurse into another domain's SPF.
    Include {
        /// Qualifier applied on match.
        qualifier: Qualifier,
        /// Included domain. `CompactString` per `A.domain`.
        domain: CompactString,
    },
    /// `exists:%{ir}.example.com` — match if the lookup returns ANY A.
    Exists {
        /// Qualifier applied on match.
        qualifier: Qualifier,
        /// Domain template to look up. Macro expansion is out of v1 scope;
        /// the literal template is used as-is. `CompactString` per `A.domain`.
        domain: CompactString,
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
        // Single-pass byte iterator over the input. Tokenisation
        // (find the next SP), modifier filter (is the token a
        // `name=value` modifier rather than a `mech:value`?) and
        // per-token parsing are all driven from the same forward
        // walk — no `str::split(' ')` iterator intermediate, no
        // `token.contains('=')` second pass over each token.
        //
        // Same architectural shape as mail-auth 0.9's
        // `TxtRecordParser::parse(bytes)`, which uses a stateful
        // `bytes.iter()` + `next_term()` driver.
        let trimmed = input.trim();
        let after_version = trimmed
            .strip_prefix("v=spf1")
            .ok_or_else(|| SpfError::InvalidRecord("missing v=spf1 prefix".into()))?;

        let bytes = after_version.as_bytes();
        let mut mechanisms = Vec::with_capacity(4);

        // tok_start: start of the current token's bytes (or end-of-input
        // if we're between tokens).
        let mut pos = 0;
        while pos < bytes.len() {
            // Skip leading SP — typically just one between mechanisms.
            while pos < bytes.len() && bytes[pos] == b' ' {
                pos += 1;
            }
            if pos >= bytes.len() {
                break;
            }
            // Find end of token via memchr (SIMD on aarch64 / x86_64).
            let tok_start = pos;
            let tok_end = match memchr::memchr(b' ', &bytes[tok_start..]) {
                Some(rel) => tok_start + rel,
                None => bytes.len(),
            };
            // Decide: modifier (skip) or mechanism (parse)? A token is
            // a modifier iff it has '=' BEFORE any ':'. We compute both
            // positions inline in the same byte walk; saves a second
            // pass for the simple-record common case where neither
            // char is present in the token (e.g. `-all`).
            let token_bytes = &bytes[tok_start..tok_end];
            let eq = memchr::memchr(b'=', token_bytes);
            let colon = memchr::memchr(b':', token_bytes);
            let is_modifier = match (eq, colon) {
                (Some(e), Some(c)) => e < c,
                (Some(_), None) => true,
                _ => false,
            };
            if !is_modifier {
                // SAFETY: bytes come from a valid `&str` and we only
                // sliced on memchr-found byte boundaries, all ASCII.
                let token = unsafe { std::str::from_utf8_unchecked(token_bytes) };

                // Inline fast-path for the bare `all` mechanism — by
                // far the most common token in a simple SPF record
                // (every record ends with `-all`, `~all`, `?all`, or
                // `+all`). Skips the parse_mechanism call entirely.
                let tb = token.as_bytes();
                let inline_all = matches!(tb, b"all" | b"+all" | b"-all" | b"~all" | b"?all");
                if inline_all {
                    let qualifier = if tb.len() == 4 {
                        match tb[0] {
                            b'+' => Qualifier::Pass,
                            b'-' => Qualifier::Fail,
                            b'~' => Qualifier::SoftFail,
                            b'?' => Qualifier::Neutral,
                            _ => Qualifier::Pass, // unreachable
                        }
                    } else {
                        Qualifier::Pass
                    };
                    mechanisms.push(Mechanism::All { qualifier });
                } else {
                    mechanisms.push(parse_mechanism(token)?);
                }
            }
            pos = tok_end + 1;
        }

        Ok(Record { mechanisms })
    }
}
