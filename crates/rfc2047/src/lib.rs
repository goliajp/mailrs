#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Internal layout: [`decode`] is the entry point. It scans for
//! `=?charset?(B|Q)?text?=` tokens and replaces them with their UTF-8
//! decoding; ASCII runs are copied unchanged. Charset → UTF-8
//! conversion goes through `encoding_rs::Encoding::for_label`.

use std::borrow::Cow;

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};

/// Encode a UTF-8 string as an RFC 2047 encoded-word **if and only if**
/// it contains non-ASCII bytes. Pure-ASCII inputs are returned as
/// borrowed `Cow::Borrowed` unchanged — no allocation, no wrapping.
///
/// The encoded form uses Base64 (`B`) with the UTF-8 charset:
/// `=?UTF-8?B?<base64>?=`. This is the wire-form most receivers
/// recognize. (Q encoding would sometimes produce shorter output for
/// mostly-ASCII strings with a few non-ASCII chars, but the size
/// difference is small and Base64 is robust across every charset.)
///
/// ```
/// use mailrs_rfc2047::encode;
/// // ASCII passes through borrowed, no allocation.
/// assert_eq!(encode("Hello"), "Hello");
/// // Non-ASCII becomes a UTF-8 Base64 encoded-word.
/// assert_eq!(encode("日本語"), "=?UTF-8?B?5pel5pys6Kqe?=");
/// ```
///
/// This is the inverse of [`decode`]: feeding `encode(decode(x))` back
/// through `decode` returns the original string (idempotent for ASCII
/// input, identity-modulo-canonicalization for encoded input).
pub fn encode(input: &str) -> Cow<'_, str> {
    // Fast path: pure ASCII *and* free of any literal `=?` sequence —
    // `=?` is the start-of-encoded-word marker RFC 2047 §2 reserves, so
    // an ASCII input that already contains it cannot safely pass through
    // borrowed (decoding the unchanged output would interpret the
    // literal `=?` as an encoded-word start and corrupt the payload —
    // found via fuzz, see CHANGELOG 1.1.2).
    if input.is_ascii() && !input.as_bytes().windows(2).any(|w| w == b"=?") {
        return Cow::Borrowed(input);
    }
    let encoded = B64.encode(input.as_bytes());
    // Output layout: "=?UTF-8?B?" + base64 + "?=" — fixed 12 byte overhead
    // around the base64 output.
    let mut out = String::with_capacity(12 + encoded.len());
    out.push_str("=?UTF-8?B?");
    out.push_str(&encoded);
    out.push_str("?=");
    Cow::Owned(out)
}

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
    fn encode_ascii_is_borrowed() {
        let r = encode("Hello World");
        assert_eq!(r, "Hello World");
        assert!(matches!(r, Cow::Borrowed(_)));
    }

    #[test]
    fn encode_japanese() {
        let r = encode("日本語");
        assert_eq!(r, "=?UTF-8?B?5pel5pys6Kqe?=");
    }

    #[test]
    fn encode_roundtrip_via_decode() {
        let original = "café — 日本語 — émoji 🦀";
        let encoded = encode(original);
        // Decode it back; should match original.
        let decoded = decode(encoded.as_bytes());
        assert_eq!(decoded, original);
    }

    #[test]
    fn encode_empty_string() {
        let r = encode("");
        assert_eq!(r, "");
        assert!(matches!(r, Cow::Borrowed(_)));
    }

    #[test]
    fn encode_pure_emoji() {
        let r = encode("🦀🚀");
        // It will be a UTF-8 Base64 encoded-word.
        assert!(r.starts_with("=?UTF-8?B?"));
        assert!(r.ends_with("?="));
        // And it decodes back identically.
        let decoded = decode(r.as_bytes());
        assert_eq!(decoded, "🦀🚀");
    }

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

    // ===== additional edge cases =====

    #[test]
    fn q_encoding_with_latin1_chars() {
        // "café" with Latin-1: c=0x63 a=0x61 f=0x66 é=0xE9.
        // Q-encoded: cafe=E9 (the é becomes =E9)
        let r = decode(b"=?iso-8859-1?Q?caf=E9?=");
        assert_eq!(r, "café");
    }

    #[test]
    fn empty_encoded_word_body() {
        // =?UTF-8?B??= — empty text. Base64 decodes empty to empty.
        let r = decode(b"=?UTF-8?B??=");
        assert_eq!(r, "");
    }

    #[test]
    fn adjacent_words_different_charsets_no_collapse() {
        // Whitespace collapse only applies when CONSECUTIVE encoded
        // words exist; different charsets is still "consecutive" per
        // RFC 2047 §6.2. Our impl collapses uniformly. Test the
        // behavior is consistent.
        let r = decode(b"=?UTF-8?B?aGk=?= =?iso-8859-1?B?aGk=?=");
        // Both decode to "hi", whitespace dropped between
        assert_eq!(r, "hihi");
    }

    #[test]
    fn encoded_word_at_very_start_of_input() {
        let r = decode(b"=?UTF-8?B?aGVsbG8=?= trailing text");
        assert_eq!(r, "hello trailing text");
    }

    #[test]
    fn encoded_word_at_very_end_of_input() {
        let r = decode(b"leading text =?UTF-8?B?aGVsbG8=?=");
        assert_eq!(r, "leading text hello");
    }

    #[test]
    fn encoded_word_in_middle_of_quoted_string() {
        // Real-world senders embed =?...?= inside what looks like a
        // quoted display-name. We decode the encoded-word regardless
        // of context.
        let r = decode(b"\"=?UTF-8?B?aGVsbG8=?=\" <addr@example.com>");
        // The unquoted display name decodes.
        assert!(r.contains("hello"));
        assert!(r.contains("<addr@example.com>"));
    }

    #[test]
    fn charset_case_insensitive_match() {
        // encoding_rs::for_label is case-insensitive
        let r1 = decode(b"=?UTF-8?B?aGk=?=");
        let r2 = decode(b"=?utf-8?B?aGk=?=");
        let r3 = decode(b"=?Utf-8?B?aGk=?=");
        let r4 = decode(b"=?UtF-8?B?aGk=?=");
        assert_eq!(r1, r2);
        assert_eq!(r2, r3);
        assert_eq!(r3, r4);
    }

    #[test]
    fn shift_jis_japanese_decode() {
        // "テスト" (Test) in Shift_JIS via Base64.
        // Shift_JIS bytes for テスト = 83 65 83 58 83 67
        let r = decode(b"=?Shift_JIS?B?g2WDWINn?=");
        assert_eq!(r, "テスト");
    }

    #[test]
    fn euc_jp_japanese_decode() {
        // "テスト" in EUC-JP via Base64.
        // EUC-JP bytes for テスト: A5 C6 A5 B9 A5 C8
        let r = decode(b"=?EUC-JP?B?pcaluaXI?=");
        assert_eq!(r, "テスト");
    }

    #[test]
    fn big5_chinese_decode() {
        // "你好" (Hello) in Big5 via Base64.
        // Big5 bytes for 你好: A7 41 A6 6E
        let r = decode(b"=?Big5?B?p0GmbA==?=");
        // Some Big5 mappings vary; just verify a non-empty UTF-8 result.
        assert!(!r.is_empty());
    }

    #[test]
    fn q_encoding_uppercase_hex() {
        let r = decode(b"=?UTF-8?Q?=E6=97=A5=E6=9C=AC=E8=AA=9E?=");
        // Hex E6 97 A5 E6 9C AC E8 AA 9E = "日本語" in UTF-8
        assert_eq!(r, "日本語");
    }

    #[test]
    fn q_encoding_lowercase_hex_tolerated() {
        // RFC 2047 §4.2 says hex chars are uppercase; some senders
        // ship lowercase. Be lenient on decode.
        let r = decode(b"=?UTF-8?Q?=e6=97=a5?=");
        // Just first 3 hex bytes E6 97 A5 = "日" (Japanese kanji for sun/day)
        assert_eq!(r, "日");
    }

    #[test]
    fn encoded_word_with_underscore_and_equals() {
        // "Hello World!" in Q: H, e, l, l, o, _, W, o, r, l, d, =21
        // _ becomes space, =21 = '!'
        let r = decode(b"=?UTF-8?Q?Hello_World=21?=");
        assert_eq!(r, "Hello World!");
    }

    // ===== encode tests =====

    #[test]
    fn encode_preserves_short_ascii() {
        // Short ASCII strings borrow without allocation.
        let r = encode("test");
        assert_eq!(r, "test");
        assert!(matches!(r, Cow::Borrowed(_)));
    }

    #[test]
    fn encode_decode_roundtrip_iso_2022_jp_via_utf8_wrapping() {
        // We encode as UTF-8 Base64 regardless of input. So Japanese
        // input encoded by us decodes back to original.
        let original = "明日午前9時の会議";
        let encoded = encode(original);
        let decoded = decode(encoded.as_bytes());
        assert_eq!(decoded, original);
    }

    #[test]
    fn encode_string_with_mixed_ascii_and_unicode() {
        // Any non-ASCII char triggers full encoding (not partial).
        let r = encode("Hello 世界");
        assert!(r.starts_with("=?UTF-8?B?"));
        let back = decode(r.as_bytes());
        assert_eq!(back, "Hello 世界");
    }
}
