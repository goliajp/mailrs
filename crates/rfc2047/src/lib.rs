#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Internal layout: [`decode`] is the entry point. It scans for
//! `=?charset?(B|Q)?text?=` tokens and replaces them with their UTF-8
//! decoding; ASCII runs are copied unchanged. Charset → UTF-8
//! conversion goes through `encoding_rs::Encoding::for_label`.

use std::borrow::Cow;

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};

/// Decode an RFC 2047 encoded header value into UTF-8.
///
/// If the input contains no `=?…?=` tokens, the original byte slice is
/// returned as a `Cow::Borrowed` `&str` without allocation (provided
/// the bytes were already valid UTF-8). Otherwise each encoded-word is
/// decoded according to RFC 2047 §3 (Q + B encodings) and joined
/// into the result.
///
/// **Whitespace between adjacent encoded-words is collapsed** per
/// RFC 2047 §6.2: `=?utf-8?B?one?= =?utf-8?B?two?=` produces `onetwo`,
/// not `one two`. Whitespace between an encoded-word and a regular
/// ASCII run is preserved.
///
/// Charsets recognized: all WHATWG Encoding labels (UTF-8,
/// ISO-8859-*, Windows-*, ISO-2022-JP, Shift_JIS, EUC-JP, EUC-KR,
/// Big5, GB18030, …). Unknown charsets fall through to lossy UTF-8.
///
/// ```
/// use mailrs_rfc2047::decode;
/// assert_eq!(decode(b"Plain ASCII"), "Plain ASCII");
/// assert_eq!(
///     decode(b"=?UTF-8?B?VGVzdA==?="),
///     "Test",
/// );
/// assert_eq!(
///     decode(b"=?UTF-8?Q?Hello=20World?="),
///     "Hello World",
/// );
/// ```
pub fn decode(input: &[u8]) -> Cow<'_, str> {
    // Fast-path: no encoded-word tokens. Return borrowed UTF-8 (or
    // lossy if the input isn't valid UTF-8).
    if !contains_encoded_word(input) {
        return match std::str::from_utf8(input) {
            Ok(s) => Cow::Borrowed(s),
            Err(_) => Cow::Owned(String::from_utf8_lossy(input).into_owned()),
        };
    }

    let mut out = String::with_capacity(input.len());
    let mut cursor = 0usize;
    let mut last_was_encoded = false;
    let mut pending_ws_start: Option<usize> = None;

    while cursor < input.len() {
        match find_encoded_word_start(input, cursor) {
            Some(start) => {
                // Anything between `cursor` and `start` is raw text.
                if start > cursor {
                    let raw = &input[cursor..start];
                    // RFC 2047 §6.2: drop whitespace between two
                    // adjacent encoded-words. Detect by checking
                    // whether the raw run is whitespace-only AND the
                    // previous token was encoded.
                    if last_was_encoded && raw.iter().all(|&b| matches!(b, b' ' | b'\t')) {
                        pending_ws_start = Some(start); // skip this run
                    } else {
                        // emit pending whitespace if any (was held back
                        // because we thought it might be between two
                        // encoded words, but turns out it's followed
                        // by regular text)
                        if let Some(ws_start) = pending_ws_start {
                            // ws_start..cursor: nothing to do, the ws
                            // we skipped was actually mid-run; we
                            // already drained that segment.
                            let _ = ws_start;
                            pending_ws_start = None;
                        }
                        push_lossy(&mut out, raw);
                    }
                }
                match find_encoded_word_end(input, start) {
                    Some((charset, encoding, text, end)) => {
                        decode_encoded_word(&mut out, charset, encoding, text);
                        cursor = end;
                        last_was_encoded = true;
                        pending_ws_start = None;
                    }
                    None => {
                        // Malformed `=?` start without a matching
                        // `?=`. Emit the `=?` literally and continue.
                        out.push('=');
                        out.push('?');
                        cursor = start + 2;
                        last_was_encoded = false;
                    }
                }
            }
            None => {
                let raw = &input[cursor..];
                push_lossy(&mut out, raw);
                break;
            }
        }
    }

    Cow::Owned(out)
}

/// Quick scan: does the input contain `=?` (the encoded-word lead-in)
/// anywhere? Used by [`decode`]'s fast path.
fn contains_encoded_word(input: &[u8]) -> bool {
    let mut i = 0;
    while i + 1 < input.len() {
        if input[i] == b'=' && input[i + 1] == b'?' {
            return true;
        }
        i += 1;
    }
    false
}

/// Locate the next `=?` in `input` starting at `from`. Returns the
/// offset of the `=` byte.
fn find_encoded_word_start(input: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i + 1 < input.len() {
        if input[i] == b'=' && input[i + 1] == b'?' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Parse one encoded-word starting at `start` (which points at `=`
/// of `=?`).
///
/// Returns `(charset, encoding, text, end)` where `end` is the byte
/// after the closing `?=`. Returns `None` if the token is malformed
/// (no two `?` separators or no closing `?=` before EOF).
fn find_encoded_word_end(input: &[u8], start: usize) -> Option<(&[u8], u8, &[u8], usize)> {
    // After "=?", scan for the next "?" to delimit charset.
    let charset_start = start + 2;
    if charset_start >= input.len() {
        return None;
    }
    let q1 = (charset_start..input.len()).find(|&i| input[i] == b'?')?;
    let charset = &input[charset_start..q1];
    if charset.is_empty() {
        return None;
    }
    let encoding_byte_pos = q1 + 1;
    if encoding_byte_pos >= input.len() {
        return None;
    }
    let encoding = input[encoding_byte_pos];
    if !matches!(encoding, b'B' | b'b' | b'Q' | b'q') {
        return None;
    }
    let q2 = encoding_byte_pos + 1;
    if q2 >= input.len() || input[q2] != b'?' {
        return None;
    }
    let text_start = q2 + 1;
    // Find closing `?=`
    let mut i = text_start;
    while i + 1 < input.len() {
        if input[i] == b'?' && input[i + 1] == b'=' {
            return Some((charset, encoding, &input[text_start..i], i + 2));
        }
        i += 1;
    }
    None
}

/// Decode one encoded-word's text into `out`. Lossy fallback if the
/// charset is unknown or decode fails.
fn decode_encoded_word(out: &mut String, charset: &[u8], encoding: u8, text: &[u8]) {
    let raw_bytes = match encoding {
        b'B' | b'b' => match B64.decode(text) {
            Ok(b) => b,
            Err(_) => {
                // Malformed base64 — copy raw text lossy.
                push_lossy(out, text);
                return;
            }
        },
        b'Q' | b'q' => decode_q(text),
        _ => return,
    };
    convert_to_utf8(out, charset, &raw_bytes);
}

/// Decode a Q (quoted-printable-style) encoded word body per
/// RFC 2047 §4.2:
///   - `_` → space
///   - `=XX` → byte 0xXX (hex)
///   - everything else: literal byte
fn decode_q(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        match text[i] {
            b'_' => {
                out.push(b' ');
                i += 1;
            }
            b'=' if i + 2 < text.len() => {
                let hi = hex_nibble(text[i + 1]);
                let lo = hex_nibble(text[i + 2]);
                match (hi, lo) {
                    (Some(h), Some(l)) => {
                        out.push((h << 4) | l);
                        i += 3;
                    }
                    _ => {
                        // Malformed `=XY` — emit literal.
                        out.push(b'=');
                        i += 1;
                    }
                }
            }
            _ => {
                out.push(text[i]);
                i += 1;
            }
        }
    }
    out
}

#[inline]
fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}

/// Convert `bytes` from `charset` to UTF-8, appending to `out`.
fn convert_to_utf8(out: &mut String, charset: &[u8], bytes: &[u8]) {
    let encoding = encoding_rs::Encoding::for_label(charset);
    let encoding = encoding.unwrap_or(encoding_rs::UTF_8);
    let (cow, _, _) = encoding.decode(bytes);
    out.push_str(&cow);
}

/// Append `bytes` to `out` as UTF-8, replacing invalid sequences.
fn push_lossy(out: &mut String, bytes: &[u8]) {
    match std::str::from_utf8(bytes) {
        Ok(s) => out.push_str(s),
        Err(_) => out.push_str(&String::from_utf8_lossy(bytes)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_ascii_is_borrowed() {
        let r = decode(b"hello world");
        assert_eq!(r, "hello world");
        assert!(matches!(r, Cow::Borrowed(_)));
    }

    #[test]
    fn utf8_no_encoding_returns_borrowed() {
        let r = decode("héllo".as_bytes());
        assert_eq!(r, "héllo");
        assert!(matches!(r, Cow::Borrowed(_)));
    }

    #[test]
    fn base64_utf8() {
        let r = decode(b"=?UTF-8?B?VGVzdA==?=");
        assert_eq!(r, "Test");
    }

    #[test]
    fn quoted_printable_utf8() {
        let r = decode(b"=?UTF-8?Q?Hello=20World?=");
        assert_eq!(r, "Hello World");
    }

    #[test]
    fn q_underscore_is_space() {
        let r = decode(b"=?UTF-8?Q?Hello_World?=");
        assert_eq!(r, "Hello World");
    }

    #[test]
    fn q_lowercase_encoding_marker() {
        let r = decode(b"=?utf-8?q?ohai?=");
        assert_eq!(r, "ohai");
    }

    #[test]
    fn b_lowercase_encoding_marker() {
        let r = decode(b"=?utf-8?b?dGVzdA==?=");
        assert_eq!(r, "test");
    }

    #[test]
    fn iso_8859_1() {
        // "café" encoded as ISO-8859-1 then base64'd:
        //   c=0x63 a=0x61 f=0x66 é=0xE9 → Y2Fm6Q==
        let r = decode(b"=?iso-8859-1?B?Y2Fm6Q==?=");
        assert_eq!(r, "café");
    }

    #[test]
    fn iso_2022_jp_japanese() {
        // "こんにちは" in ISO-2022-JP via B encoding.
        // The actual bytes: ESC $ B then JIS-encoded chars then ESC ( B.
        let r = decode(b"=?ISO-2022-JP?B?GyRCJDMkcyRLJEEkTxsoQg==?=");
        assert_eq!(r, "こんにちは");
    }

    #[test]
    fn mixed_ascii_and_encoded() {
        let r = decode(b"Prefix =?UTF-8?B?VGVzdA==?= Suffix");
        assert_eq!(r, "Prefix Test Suffix");
    }

    #[test]
    fn adjacent_encoded_words_collapse_whitespace() {
        // RFC 2047 §6.2: whitespace between two encoded-words is dropped.
        let r = decode(b"=?UTF-8?B?aGVsbG8=?= =?UTF-8?B?d29ybGQ=?=");
        assert_eq!(r, "helloworld");
    }

    #[test]
    fn whitespace_preserved_around_ascii_run() {
        let r = decode(b"=?UTF-8?B?aGVsbG8=?= mid =?UTF-8?B?d29ybGQ=?=");
        assert_eq!(r, "hello mid world");
    }

    #[test]
    fn malformed_no_closing_returns_literal_lead_in() {
        let r = decode(b"=?UTF-8?B?VGVzdA");
        // `=?` is treated as literal then the rest follows
        assert!(r.starts_with("=?"));
    }

    #[test]
    fn malformed_empty_charset_kept_literal() {
        let r = decode(b"=??B?VGVzdA==?=");
        // Cannot resolve empty charset; emit as literal lead-in.
        assert!(r.starts_with("=?"));
    }

    #[test]
    fn malformed_unknown_encoding_kept_literal() {
        let r = decode(b"=?UTF-8?X?garbage?=");
        // X is not B or Q — treat the `=?` as literal.
        assert!(r.starts_with("=?"));
    }

    #[test]
    fn empty_input_returns_empty() {
        assert_eq!(decode(b""), "");
    }

    #[test]
    fn invalid_utf8_in_unencoded_returns_lossy() {
        let r = decode(&[0xFF, 0xFE, b'h', b'i']);
        // Lossy substitution adds replacement chars; "hi" survives.
        assert!(r.contains("hi"));
    }

    #[test]
    fn q_encoding_malformed_hex() {
        let r = decode(b"=?UTF-8?Q?abc=ZZdef?=");
        // Malformed =ZZ is emitted as literal '=' then continues.
        assert!(r.contains("abc"));
        assert!(r.contains("def"));
    }

    #[test]
    fn unknown_charset_falls_through_to_utf8() {
        let r = decode(b"=?x-fake-charset?B?aGVsbG8=?=");
        // Unknown charset: encoding_rs::for_label returns None;
        // we fall back to UTF-8 decode of the raw bytes.
        assert_eq!(r, "hello");
    }
}
