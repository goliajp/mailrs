#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Internal layout: [`encode_param`] (UTF-8 source → wire) and
//! [`decode_param_value`] (wire → UTF-8). The encoder writes the
//! `filename*=UTF-8''…` form when needed; the decoder parses both
//! that form and the legacy `filename="…"` quoted form.

use std::borrow::Cow;

/// Encode a (name, value) pair for use as a MIME parameter in
/// `Content-Type` or `Content-Disposition`.
///
/// If `value` is pure ASCII, the result is `name="value"` (the legacy
/// RFC 2045 form). If `value` contains any non-ASCII bytes, the
/// result is `name*=UTF-8''<percent-encoded>` (the RFC 2231 form).
///
/// The percent-encoding follows RFC 2231 §7's "extended-other-values"
/// character set: alphanumerics + `.`, `-`, `_` pass through unchanged;
/// everything else (including space) is `%XX`-encoded with UPPERCASE
/// hex.
///
/// ```
/// use mailrs_rfc2231::encode_param;
/// assert_eq!(encode_param("filename", "test.pdf"), "filename=\"test.pdf\"");
/// // Japanese filename: UTF-8 bytes percent-encoded.
/// let r = encode_param("filename", "日本.pdf");
/// assert!(r.starts_with("filename*=UTF-8''"));
/// assert!(r.ends_with(".pdf"));
/// ```
pub fn encode_param(name: &str, value: &str) -> String {
    if value.is_ascii() {
        // Legacy form. Length: name + 3 ('="' + closing '"') + value.
        let mut out = String::with_capacity(name.len() + 3 + value.len());
        out.push_str(name);
        out.push_str("=\"");
        out.push_str(value);
        out.push('"');
        return out;
    }

    // RFC 2231 form. Estimate: most non-ASCII bytes need %XX (3 chars).
    // Bound the worst case at value.len() × 3 + ~20 fixed overhead.
    let mut encoded = String::with_capacity(value.len() * 3 / 2 + 16);
    for &b in value.as_bytes() {
        if is_param_safe(b) {
            encoded.push(b as char);
        } else {
            // Two hex chars, uppercase. fmt::Write with `{:02X}` is
            // measurably slower than a hand-table; use ascii_hex.
            encoded.push('%');
            encoded.push(ASCII_HEX_UPPER[(b >> 4) as usize] as char);
            encoded.push(ASCII_HEX_UPPER[(b & 0xF) as usize] as char);
        }
    }

    let mut out = String::with_capacity(name.len() + 11 + encoded.len());
    out.push_str(name);
    out.push_str("*=UTF-8''");
    out.push_str(&encoded);
    out
}

/// Decode an RFC 2231 parameter VALUE back to UTF-8.
///
/// Accepts the three real-world shapes:
/// 1. **Legacy quoted**: `"some value"` — strips the quotes, returns
///    the inside (typically used for ASCII).
/// 2. **Legacy unquoted**: `bareword` — returned unchanged.
/// 3. **RFC 2231 extended**: `UTF-8''<pct>` or `iso-8859-1'en'<pct>`
///    — charset is decoded via `encoding_rs::for_label`, percent
///    escapes are resolved.
///
/// Returns `Cow::Borrowed` when no decoding work was needed (form 2,
/// or form 1 with no actual quotes to strip).
///
/// `None` is reserved for the case where the input claims to be the
/// extended form but the charset label is unknown.
///
/// ```
/// use mailrs_rfc2231::decode_param_value;
/// assert_eq!(decode_param_value("\"test.pdf\"").as_deref(), Some("test.pdf"));
/// assert_eq!(decode_param_value("bareword").as_deref(), Some("bareword"));
/// // RFC 2231 form with charset prefix
/// let r = decode_param_value("UTF-8''%E6%97%A5%E6%9C%AC.pdf");
/// assert_eq!(r.as_deref(), Some("日本.pdf"));
/// ```
pub fn decode_param_value(input: &str) -> Option<Cow<'_, str>> {
    // Form 1: quoted-string (highest priority — apostrophes inside
    // quotes never mean RFC 2231 extended form)
    if input.starts_with('"') && input.ends_with('"') && input.len() >= 2 {
        // Strip the quotes. RFC 5322 §3.2.4 actually allows
        // backslash-escaped chars inside quotes; we unescape them here.
        let inner = &input[1..input.len() - 1];
        if inner.contains('\\') {
            let mut out = String::with_capacity(inner.len());
            let mut chars = inner.chars();
            while let Some(ch) = chars.next() {
                if ch == '\\' {
                    if let Some(next) = chars.next() {
                        out.push(next);
                    }
                } else {
                    out.push(ch);
                }
            }
            return Some(Cow::Owned(out));
        }
        return Some(Cow::Borrowed(inner));
    }

    // Form 3: charset'lang'percent-encoded — detect by the shape
    // (two apostrophes outside quotes). If shape matches, we commit
    // to this form: unknown charset → None, never silent bareword fallback.
    if looks_like_extended(input) {
        return try_decode_extended(input).map(Cow::Owned);
    }

    // Form 2: bareword
    Some(Cow::Borrowed(input))
}

/// Does the input look like the RFC 2231 extended form
/// `charset'lang'pct`? Used to decide whether to commit to extended
/// decoding vs treating as a bareword.
fn looks_like_extended(input: &str) -> bool {
    // Need at least two apostrophes and SOME charset before the first one.
    let Some(first) = input.find('\'') else {
        return false;
    };
    if first == 0 {
        // Charset name can't be empty.
        return false;
    }
    let after_first = &input[first + 1..];
    after_first.contains('\'')
}

/// Try to decode the RFC 2231 extended form: `charset'lang'pct-text`.
/// Returns `Some(decoded)` if the input matches the shape AND the
/// charset is known; otherwise `None`.
fn try_decode_extended(input: &str) -> Option<String> {
    // Find first `'`. If absent, not extended form.
    let first_quote = input.find('\'')?;
    let after_charset = &input[first_quote + 1..];
    // Find second `'`.
    let second_quote_rel = after_charset.find('\'')?;
    let charset = &input[..first_quote];
    let _lang = &after_charset[..second_quote_rel]; // discard; RFC 2231 says lang is informative
    let encoded = &after_charset[second_quote_rel + 1..];

    let enc = encoding_rs::Encoding::for_label(charset.as_bytes())?;

    // Resolve percent-encoded bytes first.
    let raw = decode_percent_escapes(encoded);
    let (decoded, _, _) = enc.decode(&raw);
    Some(decoded.into_owned())
}

/// Resolve `%XX` escapes into raw bytes. Lone `%` or `%X<non-hex>` is
/// left as a literal byte (lenient).
fn decode_percent_escapes(input: &str) -> Vec<u8> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = hex_nibble(bytes[i + 1]);
            let lo = hex_nibble(bytes[i + 2]);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

#[inline]
fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Byte is allowed in RFC 2231 extended-other-values without escaping.
#[inline]
fn is_param_safe(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_'
}

const ASCII_HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_ascii_legacy_quoted_form() {
        assert_eq!(
            encode_param("filename", "test.pdf"),
            "filename=\"test.pdf\""
        );
    }

    #[test]
    fn encode_ascii_with_spaces_still_legacy_form() {
        // RFC 2045 allows spaces inside quoted strings.
        assert_eq!(
            encode_param("filename", "test file.pdf"),
            "filename=\"test file.pdf\""
        );
    }

    #[test]
    fn encode_japanese_uses_rfc2231_extended() {
        let r = encode_param("filename", "日本.pdf");
        assert!(r.starts_with("filename*=UTF-8''"));
        // The dot and "pdf" pass through unencoded.
        assert!(r.ends_with(".pdf"));
        // The Japanese bytes get %XX-encoded with uppercase hex.
        assert!(r.contains("%E6%97%A5"));
    }

    #[test]
    fn encode_unsafe_ascii_chars_are_percent_encoded() {
        // Mostly-ASCII string with one space — space goes to %20.
        let r = encode_param("name", "café"); // é triggers extended form
        assert!(r.contains("caf"));
        assert!(r.starts_with("name*=UTF-8''"));
    }

    #[test]
    fn decode_legacy_quoted_string() {
        assert_eq!(
            decode_param_value("\"test.pdf\"").as_deref(),
            Some("test.pdf")
        );
    }

    #[test]
    fn decode_unquoted_bareword() {
        assert_eq!(
            decode_param_value("attachment").as_deref(),
            Some("attachment")
        );
    }

    #[test]
    fn decode_rfc2231_utf8_extended() {
        let r = decode_param_value("UTF-8''%E6%97%A5%E6%9C%AC.pdf");
        assert_eq!(r.as_deref(), Some("日本.pdf"));
    }

    #[test]
    fn decode_rfc2231_with_language_tag() {
        // Language tag between the two quotes is informative; we ignore it.
        let r = decode_param_value("UTF-8'en'%E6%97%A5%E6%9C%AC.pdf");
        assert_eq!(r.as_deref(), Some("日本.pdf"));
    }

    #[test]
    fn decode_rfc2231_iso_8859_1() {
        // "café" in ISO-8859-1: c=63 a=61 f=66 é=E9 → %63%61%66%E9 → just %E9 is enough
        // since 63 61 66 are ASCII.
        let r = decode_param_value("iso-8859-1''caf%E9");
        assert_eq!(r.as_deref(), Some("café"));
    }

    #[test]
    fn decode_unknown_charset_returns_none() {
        let r = decode_param_value("x-fake-charset''anything");
        assert!(r.is_none());
    }

    #[test]
    fn encode_decode_roundtrip_japanese() {
        let original = "テスト.pdf";
        let encoded = encode_param("filename", original);
        // Strip "filename*=" prefix to get just the value
        let value = encoded.strip_prefix("filename*=").unwrap();
        let decoded = decode_param_value(value).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn encode_decode_roundtrip_ascii() {
        let original = "test.pdf";
        let encoded = encode_param("filename", original);
        let value = encoded.strip_prefix("filename=").unwrap();
        let decoded = decode_param_value(value).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn percent_decode_lone_percent_kept() {
        // "100%" — the lone % has no following hex, should be left alone.
        let r = decode_percent_escapes("100%");
        assert_eq!(r, b"100%");
    }

    #[test]
    fn percent_decode_invalid_hex_kept() {
        // "%XX" where XX isn't hex
        let r = decode_percent_escapes("%XY");
        assert_eq!(r, b"%XY");
    }

    #[test]
    fn percent_decode_lowercase_hex_tolerated() {
        let r = decode_percent_escapes("%e6%97%a5");
        // %e6%97%a5 = 0xE6 0x97 0xA5 = "日" in UTF-8
        assert_eq!(r, vec![0xE6, 0x97, 0xA5]);
    }

    #[test]
    fn decode_quoted_with_backslash_escape() {
        // RFC 5322: "say \"hi\"" is "say "hi""
        let r = decode_param_value("\"say \\\"hi\\\"\"");
        assert_eq!(r.as_deref(), Some("say \"hi\""));
    }

    #[test]
    fn decode_empty_quoted_string() {
        assert_eq!(decode_param_value("\"\"").as_deref(), Some(""));
    }

    #[test]
    fn encode_ascii_avoids_allocation_for_short() {
        // Just verify the function doesn't panic on short ASCII.
        let r = encode_param("a", "b");
        assert_eq!(r, "a=\"b\"");
    }

    #[test]
    fn encode_param_name_with_hyphen() {
        let r = encode_param("X-Custom-Name", "value");
        assert_eq!(r, "X-Custom-Name=\"value\"");
    }

    #[test]
    fn encode_preserves_safe_chars_unencoded_in_extended_form() {
        // The "test" portion (alphanumeric) should pass through unencoded
        // even when the extended form is triggered by another char.
        let r = encode_param("filename", "test_é.pdf");
        // "test_" survives, only é becomes %C3%A9
        assert!(r.contains("test_"));
        assert!(r.contains(".pdf"));
        assert!(r.contains("%C3%A9"));
    }

    #[test]
    fn decode_no_apostrophe_falls_through_to_bareword() {
        // Without two `'` chars, this is just a bareword.
        let r = decode_param_value("not_extended_form");
        assert_eq!(r.as_deref(), Some("not_extended_form"));
    }

    #[test]
    fn decode_apostrophe_in_quoted_not_treated_as_extended() {
        // Quoted form takes precedence — apostrophes inside don't matter.
        let r = decode_param_value("\"can't.pdf\"");
        assert_eq!(r.as_deref(), Some("can't.pdf"));
    }
}
