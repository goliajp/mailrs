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
        while i < headers.len() && headers[i] != b'\n' {
            i += 1;
        }
        i += 1;
    }
    Err(DkimError::MissingHeader)
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
        while i < headers.len() && headers[i] != b'\n' {
            i += 1;
        }
        i += 1;
    }
    None
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
    fn clear_b_replaces_value_only() {
        let v = "i=1; a=rsa-sha256; d=ex.com; s=mail; h=From; bh=BH; b=SIG/+abc==";
        assert_eq!(
            clear_b_value(v),
            "i=1; a=rsa-sha256; d=ex.com; s=mail; h=From; bh=BH; b="
        );
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
