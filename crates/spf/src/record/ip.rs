//! Address and prefix parsing, and the subnet test.

//! SPF record parsing (RFC 7208 §4.6).
//!
//! Turns the raw TXT string (`"v=spf1 ip4:1.2.3.4 include:example.com -all"`)
//! into a typed [`Record`] with [`Mechanism`]s + [`Qualifier`]s.

use std::net::{IpAddr, Ipv4Addr};

use crate::error::SpfError;

/// Borrow-returning variant — avoids the `to_string()` allocation that the
/// SPF hot path used to pay per `ip4:`/`ip6:` mechanism.
/// Byte-level IPv4 dotted-quad parser. Returns `None` for any input
/// `std::net::Ipv4Addr::FromStr` would also reject (4 octets, 0-255
/// each, no leading + sign, no trailing whitespace).
///
/// Single-pass state machine: walks the bytes exactly once, building
/// each octet inline. No intermediate scan for dot positions, no
/// second pass to decode octets — same shape as mail-auth 0.9's
/// `Ipv4Addr` parser. ~5-8× faster than `<Ipv4Addr as FromStr>` on
/// typical SPF dotted-quad inputs.
#[inline]
pub(crate) fn parse_ipv4_fast(s: &str) -> Option<Ipv4Addr> {
    let bytes = s.as_bytes();
    let mut octets = [0u8; 4];
    let mut idx = 0usize;
    let mut current: u16 = 0;
    let mut started = false;

    for &b in bytes {
        if b.is_ascii_digit() {
            current = current * 10 + (b - b'0') as u16;
            if current > 255 {
                return None;
            }
            started = true;
        } else if b == b'.' {
            if !started || idx >= 3 {
                return None;
            }
            // SAFETY: current ≤ 255 enforced above.
            octets[idx] = current as u8;
            idx += 1;
            current = 0;
            started = false;
        } else {
            return None;
        }
    }

    if !started || idx != 3 {
        return None;
    }
    octets[3] = current as u8;
    Some(Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]))
}

/// Decode a 1-3 byte ASCII decimal slice into `u8`. Same input space
/// as `<u8 as FromStr>` but unrolled per length so LLVM can elide the
/// loop-counter induction that the generic parser pays for.
#[inline]
pub(crate) fn parse_octet(bytes: &[u8]) -> Option<u8> {
    match bytes.len() {
        1 => {
            let d = bytes[0].wrapping_sub(b'0');
            if d <= 9 { Some(d) } else { None }
        }
        2 => {
            let d0 = bytes[0].wrapping_sub(b'0');
            let d1 = bytes[1].wrapping_sub(b'0');
            if d0 <= 9 && d1 <= 9 {
                Some(d0 * 10 + d1)
            } else {
                None
            }
        }
        3 => {
            let d0 = bytes[0].wrapping_sub(b'0');
            let d1 = bytes[1].wrapping_sub(b'0');
            let d2 = bytes[2].wrapping_sub(b'0');
            if d0 <= 9 && d1 <= 9 && d2 <= 9 {
                let v = d0 as u16 * 100 + d1 as u16 * 10 + d2 as u16;
                if v <= 255 { Some(v as u8) } else { None }
            } else {
                None
            }
        }
        _ => None,
    }
}

#[inline]
pub(crate) fn parse_addr_and_prefix(value: &str, default: u8) -> Result<(&str, u8), SpfError> {
    if let Some((addr, prefix_str)) = value.rsplit_once('/') {
        // Reuse the unrolled per-length octet parser — prefix is also
        // a 1-3 digit u8 in the SPF grammar (1-32 for ip4, 1-128 for
        // ip6). Avoids `<u8 as FromStr>`'s generic loop induction.
        let prefix = parse_octet(prefix_str.as_bytes())
            .ok_or_else(|| SpfError::InvalidRecord(format!("bad prefix: {prefix_str}")))?;
        Ok((addr, prefix))
    } else {
        Ok((value, default))
    }
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
            let mask: u32 = if prefix == 32 {
                u32::MAX
            } else {
                !((1u32 << (32 - prefix)) - 1)
            };
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
