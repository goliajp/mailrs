//! Low-level byte-region helpers for navigating an RFC 5322 message
//! buffer without parsing it into owned structures.
//!
//! Lifted from the verifier so other crates in the email-auth family
//! (notably `mailrs-arc`'s AMS verify) can reuse the exact same
//! line-folding rules without duplicating the byte-level logic.

use crate::error::DkimError;

/// Locate the body offset (first byte AFTER the blank line that
/// separates headers from body). Tolerates lone-LF EOL just in case.
/// Returns `None` if no blank-line terminator is found.
pub fn find_body_offset(raw: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < raw.len() {
        // CRLF CRLF
        if i + 3 < raw.len() && &raw[i..i + 4] == b"\r\n\r\n" {
            return Some(i + 4);
        }
        // LF LF (tolerate lone-LF systems)
        if i + 1 < raw.len() && raw[i] == b'\n' && raw[i + 1] == b'\n' {
            return Some(i + 2);
        }
        i += 1;
    }
    None
}

/// Given a body offset returned by [`find_body_offset`], compute the
/// end-of-headers offset (the index one past the last header's
/// terminating CR/LF, exclusive of the blank line itself).
pub fn body_offset_minus_blank(body_offset: usize, raw: &[u8]) -> usize {
    if body_offset >= 2 && &raw[body_offset - 2..body_offset] == b"\r\n" {
        body_offset - 2
    } else if body_offset >= 1 && raw[body_offset - 1] == b'\n' {
        body_offset - 1
    } else {
        body_offset
    }
}

/// Find a header by name in a raw headers region, return its full
/// (possibly folded) value as an owned `String`. Returns
/// [`DkimError::MissingHeader`] if not found.
///
/// "Folded" means continuation lines beginning with WSP are joined
/// to the previous line's value, per RFC 5322 §2.2.3.
pub fn find_header_value_in_raw(headers: &[u8], name: &[u8]) -> Result<String, DkimError> {
    let mut i = 0;
    while i < headers.len() {
        if i + name.len() < headers.len()
            && headers[i..i + name.len()].eq_ignore_ascii_case(name)
            && headers[i + name.len()] == b':'
        {
            let value_start = i + name.len() + 1;
            let mut j = value_start;
            while j < headers.len() {
                if headers[j] == b'\n' {
                    let after = j + 1;
                    if after < headers.len() && matches!(headers[after], b' ' | b'\t') {
                        j += 1;
                        continue;
                    }
                    return Ok(String::from_utf8_lossy(&headers[value_start..j]).into_owned());
                }
                j += 1;
            }
            return Ok(String::from_utf8_lossy(&headers[value_start..j]).into_owned());
        }
        // memchr-anchored LF scan — replaces the prior per-byte loop.
        match memchr::memchr(b'\n', &headers[i..]) {
            Some(off) => i += off + 1,
            None => i = headers.len(),
        }
    }
    Err(DkimError::MissingHeader)
}

/// Find ALL header values (folded) by name in a raw headers region.
/// Returns owned `String`s in the order they appeared. Empty when
/// the header is not present.
///
/// Used for multi-signature DKIM verification: a single message can
/// carry multiple `DKIM-Signature:` headers (one from the original
/// signer, one from a mail-list forwarder, etc.) and each must be
/// verified independently.
pub fn find_all_header_values_in_raw(headers: &[u8], name: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < headers.len() {
        if i + name.len() < headers.len()
            && headers[i..i + name.len()].eq_ignore_ascii_case(name)
            && headers[i + name.len()] == b':'
        {
            let value_start = i + name.len() + 1;
            let mut j = value_start;
            while j < headers.len() {
                if headers[j] == b'\n' {
                    let after = j + 1;
                    if after < headers.len() && matches!(headers[after], b' ' | b'\t') {
                        j += 1;
                        continue;
                    }
                    out.push(String::from_utf8_lossy(&headers[value_start..j]).into_owned());
                    i = j;
                    break;
                }
                j += 1;
            }
            if j >= headers.len() {
                out.push(String::from_utf8_lossy(&headers[value_start..j]).into_owned());
                return out;
            }
        }
        // memchr-anchored LF scan — replaces the prior per-byte loop.
        match memchr::memchr(b'\n', &headers[i..]) {
            Some(off) => i += off + 1,
            None => i = headers.len(),
        }
    }
    out
}

/// Find a header value (folded) by name in a raw headers region.
/// Returns `Some(value)` on success or `None` if not found.
///
/// Borrowing variant of [`find_header_value_in_raw`] — returns a
/// borrow into `headers` without allocating, with trailing `\r`
/// stripped. Returns `None` if the byte range isn't valid UTF-8.
pub fn find_header_value<'a>(headers: &'a [u8], name: &str) -> Option<&'a str> {
    let bytes = name.as_bytes();
    let mut i = 0;
    while i < headers.len() {
        if i + bytes.len() < headers.len()
            && headers[i..i + bytes.len()].eq_ignore_ascii_case(bytes)
            && headers[i + bytes.len()] == b':'
        {
            let value_start = i + bytes.len() + 1;
            let mut j = value_start;
            while j < headers.len() {
                if headers[j] == b'\n' {
                    let after = j + 1;
                    if after < headers.len() && matches!(headers[after], b' ' | b'\t') {
                        j += 1;
                        continue;
                    }
                    let end = if j > value_start && headers[j - 1] == b'\r' {
                        j - 1
                    } else {
                        j
                    };
                    return std::str::from_utf8(&headers[value_start..end]).ok();
                }
                j += 1;
            }
            return std::str::from_utf8(&headers[value_start..j]).ok();
        }
        // memchr-anchored LF scan — replaces the prior per-byte loop.
        match memchr::memchr(b'\n', &headers[i..]) {
            Some(off) => i += off + 1,
            None => i = headers.len(),
        }
    }
    None
}

/// Per RFC 6376 §5.4.2: for each entry in `names`, consume one
/// occurrence of that header from `headers_raw`, scanning **bottom-up**
/// and tracking already-consumed positions. Returns one
/// `(name, Option<value>)` per `names` entry in the same order. `None`
/// means the message had fewer instances of that name than `names`
/// references; the caller (sign/verify) should SKIP that entry from
/// the hash input — both OpenDKIM and stalwart/mail-auth drop missing
/// h= entries rather than emit a `name:\r\n` null line.
///
/// This handles the common DKIM hardening pattern `h=From:From` (sign
/// the From header plus a "ghost" empty From to prevent header
/// injection attacks). A naive top-down `find_header_value` would
/// always return the same header for both `h=` entries and miscompute
/// the signed-header block.
pub fn collect_signed_headers(
    headers_raw: &[u8],
    names: &[String],
) -> Vec<(String, Option<String>)> {
    // Thin owned-result wrapper over the borrowing implementation
    // for external API stability — internal call sites (`sign::sign`,
    // `verifier::verify_one`) call `collect_signed_headers_borrowed`
    // directly to skip the per-occurrence String clones.
    collect_signed_headers_borrowed(headers_raw, names)
        .into_iter()
        .map(|(n, v)| (n.to_string(), v.map(|s| s.to_string())))
        .collect()
}

/// Zero-alloc-per-occurrence variant of [`collect_signed_headers`]:
/// the returned slices borrow from `headers_raw` (values) and from
/// `names` (the returned names mirror what the caller asked for, so
/// they live as long as `names`).
///
/// This is the actual hot-path implementation; the owned variant is
/// kept as a thin wrapper for downstream API stability.
///
/// Behaviour identical to [`collect_signed_headers`] including the
/// RFC 6376 §5.4.2 bottom-up consumption: repeated `h=From:From`
/// entries each consume one fresh occurrence, and an entry with no
/// matching unconsumed occurrence returns `None` (the caller MUST
/// skip it from the hash input — emitting a null `name:\r\n` would
/// corrupt the signature on the verify side).
pub fn collect_signed_headers_borrowed<'a, 'n>(
    headers_raw: &'a [u8],
    names: &'n [String],
) -> Vec<(&'n str, Option<&'a str>)> {
    // 1. Walk headers top-down, recording (name_slice, value_slice)
    //    per occurrence as byte ranges into `headers_raw`. Pointer +
    //    length only — no String allocation, no copy.
    let mut occurrences: Vec<(&'a [u8], &'a [u8])> = Vec::with_capacity(32);
    let mut i = 0;
    while i < headers_raw.len() {
        let line_start = i;
        let mut colon: Option<usize> = None;
        // Walk until end-of-line, noting first colon.
        // memchr2-anchored: jump to the next ':' or '\n', whichever
        // comes first. Record the first ':' as the colon position;
        // a '\n' ends the line.
        loop {
            match memchr::memchr2(b':', b'\n', &headers_raw[i..]) {
                Some(off) => {
                    let pos = i + off;
                    if headers_raw[pos] == b':' {
                        if colon.is_none() {
                            colon = Some(pos);
                        }
                        i = pos + 1;
                    } else {
                        i = pos;
                        break;
                    }
                }
                None => {
                    i = headers_raw.len();
                    break;
                }
            }
        }
        // i is now at '\n' or end-of-buffer.
        let value_end_first = if i > line_start && headers_raw[i.saturating_sub(1)] == b'\r' {
            i - 1
        } else {
            i
        };
        if i < headers_raw.len() {
            i += 1; // consume the \n
        }
        if let Some(colon_pos) = colon {
            let mut value_end = value_end_first;
            // Fold: consume continuation lines starting with WSP.
            while i < headers_raw.len() && matches!(headers_raw[i], b' ' | b'\t') {
                // walk to next \n
                // memchr LF scan for the folded continuation line.
                match memchr::memchr(b'\n', &headers_raw[i..]) {
                    Some(off) => i += off,
                    None => i = headers_raw.len(),
                }
                value_end = if i > line_start && headers_raw[i.saturating_sub(1)] == b'\r' {
                    i - 1
                } else {
                    i
                };
                if i < headers_raw.len() {
                    i += 1;
                }
            }
            let name_bytes = &headers_raw[line_start..colon_pos];
            let value_bytes = &headers_raw[colon_pos + 1..value_end];
            occurrences.push((name_bytes, value_bytes));
        }
        // Lines without a colon (e.g. accidental whitespace lines) get skipped.
    }

    // 2. For each requested name (in h= order), consume the next
    //    unconsumed bottom-up occurrence. Comparison is byte-level
    //    case-insensitive — avoids the `to_ascii_lowercase()` String
    //    allocation per requested name + per occurrence.
    let mut consumed = vec![false; occurrences.len()];
    let mut result = Vec::with_capacity(names.len());
    for name in names {
        let name_bytes = name.as_bytes();
        let mut found_value: Option<&'a str> = None;
        for idx in (0..occurrences.len()).rev() {
            if !consumed[idx] && occurrences[idx].0.eq_ignore_ascii_case(name_bytes) {
                consumed[idx] = true;
                found_value = std::str::from_utf8(occurrences[idx].1).ok();
                break;
            }
        }
        result.push((name.as_str(), found_value));
    }
    result
}

/// Remove the value of the `b=` tag from a signature header value,
/// leaving the `b=` itself in place. Used when computing the
/// header-hash input — the signature bytes themselves are not part
/// of what gets hashed (chicken-and-egg).
pub fn clear_b_value(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let is_b_start = if i + 1 < bytes.len() && bytes[i] == b'b' && bytes[i + 1] == b'=' {
            let mut k = i;
            while k > 0 {
                k -= 1;
                if !matches!(bytes[k], b' ' | b'\t' | b'\r' | b'\n') {
                    break;
                }
            }
            k == 0 || bytes[k] == b';'
        } else {
            false
        };
        if is_b_start {
            out.extend_from_slice(b"b=");
            i += 2;
            while i < bytes.len() && bytes[i] != b';' {
                i += 1;
            }
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_offset_simple_crlf_crlf() {
        let raw = b"From: a@b\r\n\r\nbody";
        let off = find_body_offset(raw).unwrap();
        assert_eq!(&raw[off..], b"body");
    }

    #[test]
    fn body_offset_lf_lf() {
        let raw = b"From: a@b\n\nbody";
        let off = find_body_offset(raw).unwrap();
        assert_eq!(&raw[off..], b"body");
    }

    #[test]
    fn find_header_extracts_value() {
        let headers = b"From: alice@example.com\r\nTo: bob@example.com\r\n";
        let v = find_header_value(headers, "From");
        assert_eq!(v, Some(" alice@example.com"));
    }

    #[test]
    fn find_header_handles_folded() {
        let headers = b"Subject: line1\r\n line2\r\nFrom: a@b\r\n";
        let v = find_header_value(headers, "Subject");
        assert_eq!(v, Some(" line1\r\n line2"));
    }

    #[test]
    fn collect_signed_headers_basic() {
        let headers = b"Date: today\r\nFrom: alice@example.com\r\nTo: bob@example.com\r\n";
        let names = vec!["Date".to_string(), "From".to_string(), "To".to_string()];
        let got = collect_signed_headers(headers, &names);
        assert_eq!(got.len(), 3);
        assert_eq!(got[0], ("Date".to_string(), Some(" today".to_string())));
        assert_eq!(
            got[1],
            ("From".to_string(), Some(" alice@example.com".to_string()))
        );
        assert_eq!(
            got[2],
            ("To".to_string(), Some(" bob@example.com".to_string()))
        );
    }

    #[test]
    fn collect_signed_headers_repeated_h_consumes_bottom_up() {
        // Two From headers, two h= entries: consume bottom one first,
        // then top one (RFC 6376 §5.4.2 bottom-up scan).
        let headers = b"From: first@example.com\r\nFrom: second@example.com\r\nSubject: hi\r\n";
        let names = vec!["From".to_string(), "From".to_string()];
        let got = collect_signed_headers(headers, &names);
        assert_eq!(
            got[0],
            ("From".to_string(), Some(" second@example.com".to_string()))
        );
        assert_eq!(
            got[1],
            ("From".to_string(), Some(" first@example.com".to_string()))
        );
    }

    #[test]
    fn collect_signed_headers_overcount_yields_none() {
        // Only one From, but h= references it twice — second is null.
        // This is the RFC-compliant anti-header-injection pattern.
        let headers = b"Date: today\r\nFrom: alice@example.com\r\n";
        let names = vec!["From".to_string(), "From".to_string()];
        let got = collect_signed_headers(headers, &names);
        assert_eq!(
            got[0],
            ("From".to_string(), Some(" alice@example.com".to_string()))
        );
        assert_eq!(got[1], ("From".to_string(), None));
    }

    #[test]
    fn collect_signed_headers_case_insensitive_name_match() {
        let headers = b"FROM: alice@example.com\r\n";
        let names = vec!["From".to_string()];
        let got = collect_signed_headers(headers, &names);
        assert_eq!(
            got[0],
            ("From".to_string(), Some(" alice@example.com".to_string()))
        );
    }

    #[test]
    fn collect_signed_headers_missing_header_yields_none() {
        let headers = b"From: a@b\r\n";
        let names = vec!["From".to_string(), "Reply-To".to_string()];
        let got = collect_signed_headers(headers, &names);
        assert_eq!(got[0].1.as_deref(), Some(" a@b"));
        assert_eq!(got[1].1, None);
    }

    #[test]
    fn collect_signed_headers_folded_value() {
        let headers = b"Subject: line1\r\n line2\r\n\tline3\r\nFrom: a@b\r\n";
        let names = vec!["Subject".to_string()];
        let got = collect_signed_headers(headers, &names);
        assert_eq!(got[0].1.as_deref(), Some(" line1\r\n line2\r\n\tline3"));
    }

    #[test]
    fn clear_b_replaces_value_only() {
        let v = "i=1; a=rsa-sha256; d=ex.com; s=mail; h=From; bh=BH; b=SIG/+abc==";
        assert_eq!(
            clear_b_value(v),
            "i=1; a=rsa-sha256; d=ex.com; s=mail; h=From; bh=BH; b="
        );
    }

    #[test]
    fn find_all_header_values_in_raw_zero_matches() {
        let headers = b"From: a\r\n";
        assert!(find_all_header_values_in_raw(headers, b"DKIM-Signature").is_empty());
    }

    #[test]
    fn find_all_header_values_in_raw_single_match() {
        let headers = b"DKIM-Signature: v=1; d=a.com\r\nFrom: a\r\n";
        let v = find_all_header_values_in_raw(headers, b"DKIM-Signature");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0], " v=1; d=a.com\r");
    }

    #[test]
    fn find_all_header_values_in_raw_multi_match() {
        let headers =
            b"DKIM-Signature: v=1; d=a.com\r\nFrom: a\r\nDKIM-Signature: v=1; d=b.com\r\n";
        let v = find_all_header_values_in_raw(headers, b"DKIM-Signature");
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], " v=1; d=a.com\r");
        assert_eq!(v[1], " v=1; d=b.com\r");
    }

    #[test]
    fn find_all_header_values_in_raw_handles_folded() {
        let headers = b"DKIM-Signature: v=1;\r\n d=a.com\r\nDKIM-Signature: v=1; d=b.com\r\n";
        let v = find_all_header_values_in_raw(headers, b"DKIM-Signature");
        assert_eq!(v.len(), 2);
        assert!(v[0].contains("d=a.com"));
        assert!(v[1].contains("d=b.com"));
    }

    #[test]
    fn find_header_value_in_raw_returns_owned() {
        // The owning variant does NOT strip the trailing CR before LF —
        // by design, since `find_header_value_in_raw` is also used to
        // recover the exact pre-`b=` bytes for signature input. Keep
        // this expectation in sync with that contract.
        let headers = b"DKIM-Signature: v=1; a=rsa-sha256\r\nFrom: a@b\r\n";
        let v = find_header_value_in_raw(headers, b"DKIM-Signature").unwrap();
        assert_eq!(v, " v=1; a=rsa-sha256\r");
    }
}
