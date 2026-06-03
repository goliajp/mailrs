//! Encoding helpers: CTE selection, quoted-printable, base64,
//! header folding, encoded-word header escapes.

use base64::Engine;

/// MIME Content-Transfer-Encoding choices the builder picks from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentTransferEncoding {
    /// `7bit` — body is pure ASCII with every line ≤ 998 chars (RFC
    /// 5322 §2.1.1) and no NUL bytes. Body is emitted verbatim.
    SevenBit,
    /// `8bit` — body has ≥1 high-bit byte but every line is still
    /// short and the body is text-shaped. Emitted verbatim. Use
    /// requires the receiving MTA to support 8BITMIME — most do,
    /// but for compatibility callers can prefer `QuotedPrintable`.
    EightBit,
    /// `quoted-printable` — body has high-bit bytes or long lines
    /// and is text-shaped. Output is wrapped at 76 chars per RFC
    /// 2045 §6.7.
    QuotedPrintable,
    /// `base64` — body looks binary (high non-printable density or
    /// embedded NUL). Output is 76-char wrapped per RFC 2045 §6.8.
    Base64,
}

impl ContentTransferEncoding {
    /// The header-value string used in the `Content-Transfer-Encoding:`
    /// header.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SevenBit => "7bit",
            Self::EightBit => "8bit",
            Self::QuotedPrintable => "quoted-printable",
            Self::Base64 => "base64",
        }
    }
}

/// Pick the canonical CTE for `body`.
///
/// Heuristic, in order:
/// 1. Embedded NUL or > 15 % non-printable bytes → `Base64`
///    (treated as binary).
/// 2. Any byte > 0x7F **or** any line longer than 78 chars →
///    `QuotedPrintable` (text but needs wrapping / escaping).
/// 3. Otherwise `SevenBit` (pure ASCII, short lines).
pub fn choose_cte(body: &[u8]) -> ContentTransferEncoding {
    if body.is_empty() {
        return ContentTransferEncoding::SevenBit;
    }
    // "Non-text" bytes for the base64-vs-qp decision: ASCII
    // control characters other than \t / \r / \n. We deliberately
    // do NOT count > 0x7F here — utf-8 encoded text is high-bit-
    // heavy but still text-shaped and should ride quoted-printable,
    // not base64.
    let mut control_bytes = 0usize;
    let mut has_high_bit = false;
    let mut max_line = 0usize;
    let mut cur_line = 0usize;
    let mut has_nul = false;
    for &b in body {
        if b == 0 {
            has_nul = true;
        }
        if b > 0x7F {
            has_high_bit = true;
        }
        let is_control = b < 0x20 && b != b'\t' && b != b'\r' && b != b'\n';
        if is_control || b == 0x7F {
            control_bytes += 1;
        }
        if b == b'\n' {
            if cur_line > max_line {
                max_line = cur_line;
            }
            cur_line = 0;
        } else {
            cur_line += 1;
        }
    }
    if cur_line > max_line {
        max_line = cur_line;
    }

    if has_nul || (!body.is_empty() && control_bytes * 100 / body.len() > 15) {
        return ContentTransferEncoding::Base64;
    }
    if has_high_bit || max_line > 78 {
        return ContentTransferEncoding::QuotedPrintable;
    }
    ContentTransferEncoding::SevenBit
}

/// Quoted-printable encode per RFC 2045 §6.7 with soft line breaks
/// every 76 chars (`=\r\n`). Input is treated as bytes; output is
/// ASCII-safe.
pub fn encode_quoted_printable(body: &[u8]) -> String {
    let mut out = String::with_capacity(body.len() + body.len() / 3);
    let mut line_len = 0usize;

    fn needs_escape(b: u8, _at_eol: bool) -> bool {
        // RFC 2045 §6.7: bytes 33-60 and 62-126 may be sent
        // verbatim. Tab (0x09) and space (0x20) may also be
        // verbatim except at end-of-line; we handle EOL whitespace
        // by always escaping trailing SP/TAB.
        matches!(b, 33..=60 | 62..=126)
    }

    let push_soft_break = |out: &mut String, line_len: &mut usize| {
        out.push_str("=\r\n");
        *line_len = 0;
    };

    let mut iter = body.iter().peekable();
    while let Some(&b) = iter.next() {
        // CRLF in input is preserved as a hard line break.
        if b == b'\r' && iter.peek() == Some(&&b'\n') {
            iter.next();
            out.push_str("\r\n");
            line_len = 0;
            continue;
        }
        if b == b'\n' {
            out.push_str("\r\n");
            line_len = 0;
            continue;
        }

        // Whitespace at end-of-line must be escaped.
        let next_is_eol = matches!(iter.peek(), Some(&&b'\r' | &&b'\n') | None);
        let must_escape = if b == b' ' || b == b'\t' {
            next_is_eol
        } else {
            !needs_escape(b, false)
        };

        let chunk_len = if must_escape { 3 } else { 1 };
        if line_len + chunk_len > 75 {
            push_soft_break(&mut out, &mut line_len);
        }

        if must_escape {
            use std::fmt::Write;
            let _ = write!(out, "={b:02X}");
            line_len += 3;
        } else {
            out.push(b as char);
            line_len += 1;
        }
    }
    out
}

/// Base64-encode `body` with RFC 2045 §6.8 line breaks every 76
/// chars.
pub fn encode_base64(body: &[u8]) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(body);
    let mut out = String::with_capacity(encoded.len() + encoded.len() / 76 * 2);
    let bytes = encoded.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        let end = (idx + 76).min(bytes.len());
        out.push_str(std::str::from_utf8(&bytes[idx..end]).unwrap());
        out.push_str("\r\n");
        idx = end;
    }
    out
}

/// Fold a header value at 78 chars per RFC 5322 §2.2.3 (soft wrap
/// with CRLF + WSP continuation). The `name:` prefix is included in
/// the first line's width budget; continuation lines start with a
/// single ASCII space.
///
/// Folding happens at whitespace; if a single token is longer than
/// the soft limit (e.g. an opaque Message-ID), it is emitted on its
/// own continuation line without breaking the token itself.
pub fn fold_header(name: &str, value: &str) -> String {
    const SOFT_LIMIT: usize = 78;

    let prefix = format!("{name}: ");
    let mut out = String::with_capacity(value.len() + 8);
    out.push_str(&prefix);

    if prefix.len() + value.len() <= SOFT_LIMIT && !value.contains('\n') {
        out.push_str(value);
        return out;
    }

    let mut line_len = prefix.len();
    let mut first_token_on_line = true;
    for tok in value.split_whitespace() {
        let sep_len = if first_token_on_line { 0 } else { 1 };
        if line_len + sep_len + tok.len() > SOFT_LIMIT && !first_token_on_line {
            out.push_str("\r\n ");
            line_len = 1;
            first_token_on_line = true;
        }
        if !first_token_on_line {
            out.push(' ');
            line_len += 1;
        }
        out.push_str(tok);
        line_len += tok.len();
        first_token_on_line = false;
    }
    out
}

/// RFC 2047 encoded-word for header values that contain non-ASCII
/// bytes. ASCII-only inputs pass through unchanged. Quoting around
/// encoded-words inside structured headers (display-names in
/// `From:`, `To:`, etc.) is the caller's responsibility.
pub fn maybe_encode_word(value: &str) -> std::borrow::Cow<'_, str> {
    if value.is_ascii() {
        std::borrow::Cow::Borrowed(value)
    } else {
        mailrs_rfc2047::encode(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cte_empty_is_sevenbit() {
        assert_eq!(choose_cte(b""), ContentTransferEncoding::SevenBit);
    }

    #[test]
    fn cte_short_ascii_is_sevenbit() {
        assert_eq!(
            choose_cte(b"hello world\r\nshort line\r\n"),
            ContentTransferEncoding::SevenBit,
        );
    }

    #[test]
    fn cte_long_ascii_line_is_qp() {
        let body = format!("{}\r\n", "x".repeat(120));
        assert_eq!(
            choose_cte(body.as_bytes()),
            ContentTransferEncoding::QuotedPrintable
        );
    }

    #[test]
    fn cte_high_bit_is_qp() {
        assert_eq!(
            choose_cte("こんにちは".as_bytes()),
            ContentTransferEncoding::QuotedPrintable,
        );
    }

    #[test]
    fn cte_binary_is_base64() {
        let bytes: Vec<u8> = (0..=255u8).collect();
        assert_eq!(choose_cte(&bytes), ContentTransferEncoding::Base64);
    }

    #[test]
    fn cte_embedded_nul_is_base64() {
        assert_eq!(
            choose_cte(b"hello\x00world"),
            ContentTransferEncoding::Base64,
        );
    }

    #[test]
    fn qp_pass_through_ascii() {
        let r = encode_quoted_printable(b"hello world\r\nsecond\r\n");
        assert_eq!(r, "hello world\r\nsecond\r\n");
    }

    #[test]
    fn qp_escapes_equals_sign() {
        assert_eq!(encode_quoted_printable(b"a=b"), "a=3Db");
    }

    #[test]
    fn qp_escapes_high_bit() {
        // é = 0xC3 0xA9 in utf-8
        assert_eq!(encode_quoted_printable("é".as_bytes()), "=C3=A9");
    }

    #[test]
    fn qp_escapes_trailing_space() {
        // trailing space at end of input is EOL-adjacent
        assert_eq!(encode_quoted_printable(b"hello "), "hello=20");
    }

    #[test]
    fn qp_wraps_long_lines() {
        let body = "x".repeat(200);
        let out = encode_quoted_printable(body.as_bytes());
        // every produced line must be ≤ 76 chars (incl. trailing "=")
        for line in out.split("\r\n") {
            assert!(line.len() <= 76, "line over 76: {line:?}");
        }
    }

    #[test]
    fn base64_wraps_at_76() {
        let body = vec![0xAB; 200];
        let out = encode_base64(&body);
        for line in out.trim_end_matches("\r\n").split("\r\n") {
            assert!(line.len() <= 76, "line over 76: {line:?}");
        }
    }

    #[test]
    fn fold_short_header_unchanged() {
        let out = fold_header("Subject", "Hello world");
        assert_eq!(out, "Subject: Hello world");
        assert!(!out.contains('\n'));
    }

    #[test]
    fn fold_long_subject_wraps() {
        let value = "the quick brown fox jumps over the lazy dog and the slothful zebra and the gallant elephant";
        let out = fold_header("Subject", value);
        // every produced line must be ≤ 78 chars
        for line in out.split("\r\n") {
            assert!(line.len() <= 78, "line over 78: {line:?}");
        }
        // first line still starts with the header name
        assert!(out.starts_with("Subject: "));
        // continuation lines start with a single SP (folding WSP)
        let parts: Vec<&str> = out.split("\r\n").collect();
        for p in &parts[1..] {
            assert!(
                p.starts_with(' '),
                "continuation must start with WSP: {p:?}"
            );
        }
    }

    #[test]
    fn maybe_encode_word_ascii_pass_through() {
        let out = maybe_encode_word("Hello world");
        assert_eq!(out, "Hello world");
    }

    #[test]
    fn maybe_encode_word_non_ascii_uses_encoded_word() {
        let out = maybe_encode_word("こんにちは");
        // rfc2047::encode produces =?UTF-8?B?...?= or Q-encoded form
        assert!(out.starts_with("=?UTF-8?"));
        assert!(out.ends_with("?="));
    }
}
