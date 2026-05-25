//! Content-Transfer-Encoding decoders (RFC 2045 §6).

use base64::Engine as _;

/// The five RFC 2045 §6 transfer encodings, plus an `Other` catch-all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferEncoding {
    /// `7bit` — ASCII only, lines ≤ 998 octets. Body bytes are
    /// returned unchanged.
    SevenBit,
    /// `8bit` — full 8-bit, lines ≤ 998 octets. Body bytes unchanged.
    EightBit,
    /// `binary` — arbitrary bytes, no line constraint. Returned
    /// unchanged.
    Binary,
    /// `quoted-printable` — RFC 2045 §6.7.
    QuotedPrintable,
    /// `base64` — RFC 2045 §6.8.
    Base64,
    /// Unknown encoding name — body returned as-is with the value
    /// preserved for diagnostics.
    Other(String),
}

impl TransferEncoding {
    /// Parse the raw header value (e.g. `"quoted-printable"`,
    /// `"BASE64"`, `"7bit"`). Lowercase + trim.
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "7bit" => TransferEncoding::SevenBit,
            "8bit" => TransferEncoding::EightBit,
            "binary" => TransferEncoding::Binary,
            "quoted-printable" => TransferEncoding::QuotedPrintable,
            "base64" => TransferEncoding::Base64,
            other => TransferEncoding::Other(other.to_string()),
        }
    }

    /// Decode `body` bytes to the canonical octet stream. Per RFC 2045:
    /// `7bit`, `8bit`, `binary` pass through unchanged. `base64` and
    /// `quoted-printable` are decoded. `Other(_)` passes through.
    ///
    /// Returns `Cow::Borrowed(body)` for the identity encodings
    /// (7bit/8bit/binary/Other) so callers can keep the raw input
    /// slice without allocating. Only Base64 and Quoted-Printable
    /// produce an owned `Vec<u8>`.
    ///
    /// **API change in v4**: was `-> Vec<u8>` before. Callers that
    /// needed an owned `Vec<u8>` can wrap with `.into_owned()`.
    pub fn decode<'a>(&self, body: &'a [u8]) -> std::borrow::Cow<'a, [u8]> {
        use std::borrow::Cow;
        match self {
            TransferEncoding::SevenBit
            | TransferEncoding::EightBit
            | TransferEncoding::Binary
            | TransferEncoding::Other(_) => Cow::Borrowed(body),
            TransferEncoding::Base64 => Cow::Owned(decode_base64(body)),
            TransferEncoding::QuotedPrintable => Cow::Owned(decode_quoted_printable(body)),
        }
    }
}

/// Base64 decode, lenient: ignore whitespace, handle missing/optional
/// padding, drop invalid chars.
///
/// Fast-path: when `body` contains no whitespace (signature payloads,
/// short single-line attachments) we feed the original slice straight
/// to base64 — skipping the intermediate strip Vec entirely. memchr
/// is SIMD-vectorised so the WSP probe is faster than a per-byte
/// filter loop even when WSP IS present.
fn decode_base64(body: &[u8]) -> Vec<u8> {
    // memchr is SIMD-vectorised. Probe the four MIME WSP bytes; either
    // hit returns Some and we fall to the strip path.
    let has_wsp = memchr::memchr2(b'\n', b'\r', body).is_some()
        || memchr::memchr2(b' ', b'\t', body).is_some();
    let input: std::borrow::Cow<'_, [u8]> = if has_wsp {
        let mut cleaned = Vec::with_capacity(body.len());
        cleaned.extend(
            body.iter()
                .copied()
                .filter(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n')),
        );
        std::borrow::Cow::Owned(cleaned)
    } else {
        std::borrow::Cow::Borrowed(body)
    };
    base64::engine::general_purpose::STANDARD
        .decode(&*input)
        .unwrap_or_else(|_| {
            base64::engine::general_purpose::STANDARD_NO_PAD
                .decode(&*input)
                .unwrap_or_default()
        })
}

/// Quoted-printable decode per RFC 2045 §6.7.
///
/// - `=XX` → byte `0xXX`
/// - `=\r\n` (soft line break) → drop
/// - everything else → literal byte
fn decode_quoted_printable(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len());
    let mut i = 0;
    while i < body.len() {
        if body[i] == b'=' {
            // Soft line break: =\r\n or =\n
            if i + 1 < body.len() && body[i + 1] == b'\n' {
                i += 2;
                continue;
            }
            if i + 2 < body.len() && body[i + 1] == b'\r' && body[i + 2] == b'\n' {
                i += 3;
                continue;
            }
            // Hex escape: =XX
            if i + 2 < body.len() {
                let hi = hex_nibble(body[i + 1]);
                let lo = hex_nibble(body[i + 2]);
                if let (Some(h), Some(l)) = (hi, lo) {
                    out.push((h << 4) | l);
                    i += 3;
                    continue;
                }
            }
            // Lone `=` or malformed escape — pass through.
            out.push(b'=');
            i += 1;
            continue;
        }
        out.push(body[i]);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_canonical_names() {
        assert_eq!(TransferEncoding::parse("7bit"), TransferEncoding::SevenBit);
        assert_eq!(TransferEncoding::parse("8bit"), TransferEncoding::EightBit);
        assert_eq!(TransferEncoding::parse("binary"), TransferEncoding::Binary);
        assert_eq!(
            TransferEncoding::parse("quoted-printable"),
            TransferEncoding::QuotedPrintable
        );
        assert_eq!(TransferEncoding::parse("base64"), TransferEncoding::Base64);
    }

    #[test]
    fn parse_case_insensitive() {
        assert_eq!(TransferEncoding::parse("BASE64"), TransferEncoding::Base64);
        assert_eq!(
            TransferEncoding::parse("Quoted-Printable"),
            TransferEncoding::QuotedPrintable
        );
    }

    #[test]
    fn parse_unknown_is_other() {
        assert_eq!(
            TransferEncoding::parse("uuencode"),
            TransferEncoding::Other("uuencode".into())
        );
    }

    #[test]
    fn decode_7bit_passes_through() {
        // v4 Cow API: deref the Cow to a slice for comparison.
        assert_eq!(
            &*TransferEncoding::SevenBit.decode(b"Hello, world!\r\n"),
            b"Hello, world!\r\n"
        );
    }

    #[test]
    fn decode_base64_basic() {
        let r = TransferEncoding::Base64.decode(b"SGVsbG8gd29ybGQ=");
        assert_eq!(&*r, b"Hello world");
    }

    #[test]
    fn decode_base64_with_line_breaks() {
        let input = b"SGVsbG8s\r\nIHdvcmxkIQ==";
        let r = TransferEncoding::Base64.decode(input);
        assert_eq!(&*r, b"Hello, world!");
    }

    #[test]
    fn decode_base64_with_spaces() {
        let r = TransferEncoding::Base64.decode(b"SGVs bG8g d29y bGQ=");
        assert_eq!(&*r, b"Hello world");
    }

    #[test]
    fn decode_quoted_printable_basic() {
        let r = TransferEncoding::QuotedPrintable.decode(b"Hello=20world");
        assert_eq!(&*r, b"Hello world");
    }

    #[test]
    fn decode_quoted_printable_soft_break() {
        // `=\r\n` is a soft line break (no real newline in output)
        let r = TransferEncoding::QuotedPrintable.decode(b"long line=\r\nbreak");
        assert_eq!(&*r, b"long linebreak");
    }

    #[test]
    fn decode_quoted_printable_japanese_utf8() {
        // "日" (E6 97 A5 in UTF-8)
        let r = TransferEncoding::QuotedPrintable.decode(b"=E6=97=A5");
        assert_eq!(&*r, &[0xE6_u8, 0x97, 0xA5][..]);
    }

    #[test]
    fn decode_quoted_printable_lowercase_hex() {
        let r = TransferEncoding::QuotedPrintable.decode(b"=e6=97=a5");
        assert_eq!(&*r, &[0xE6_u8, 0x97, 0xA5][..]);
    }

    #[test]
    fn decode_quoted_printable_lone_equals_passes() {
        let r = TransferEncoding::QuotedPrintable.decode(b"100% sure");
        assert_eq!(&*r, b"100% sure");
    }

    #[test]
    fn decode_quoted_printable_invalid_hex_passes() {
        let r = TransferEncoding::QuotedPrintable.decode(b"=XY");
        assert_eq!(&*r, b"=XY");
    }

    #[test]
    fn decode_binary_passes_through_arbitrary_bytes() {
        let bytes: &[u8] = &[0x00, 0xFF, 0x80, 0x7F];
        assert_eq!(&*TransferEncoding::Binary.decode(bytes), bytes);
    }

    #[test]
    fn decode_other_passes_through() {
        let enc = TransferEncoding::Other("uuencode".into());
        assert_eq!(&*enc.decode(b"raw"), b"raw");
    }
}
