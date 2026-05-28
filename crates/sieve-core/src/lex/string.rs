//! RFC 5228 §2.4 string-literal scanners — extracted from
//! `lex/mod.rs` so the main `tokenize` loop stays under the
//! file-size limit. Both functions return the scanned `String`
//! plus the new position past the closing delimiter.

use super::TokenizeError;

/// Scan a quoted string `"..."`. Caller has already verified
/// `bytes[start] == b'"'`. Returns the unescaped contents and
/// the index just past the closing quote.
pub(super) fn scan_quoted(bytes: &[u8], start: usize) -> Result<(String, usize), TokenizeError> {
    let mut i = start + 1;
    let mut s = String::new();
    loop {
        if i >= bytes.len() {
            return Err(TokenizeError::UnterminatedString(start));
        }
        let c = bytes[i];
        if c == b'"' {
            return Ok((s, i + 1));
        }
        if c == b'\\' {
            if i + 1 >= bytes.len() {
                return Err(TokenizeError::BadEscape(i));
            }
            let esc = bytes[i + 1];
            match esc {
                b'"' => s.push('"'),
                b'\\' => s.push('\\'),
                _ => return Err(TokenizeError::BadEscape(i)),
            }
            i += 2;
            continue;
        }
        // append the actual UTF-8 code point starting at i
        let ch_len = utf8_char_len(c);
        let ch_end = i + ch_len;
        if ch_end > bytes.len() {
            return Err(TokenizeError::UnterminatedString(start));
        }
        s.push_str(std::str::from_utf8(&bytes[i..ch_end]).unwrap_or(""));
        i = ch_end;
    }
}

/// Recognise the start of a multi-line `text:` literal.
/// Returns true iff `bytes[start..]` begins with one of
/// `text:\r\n`, `text:\n`, `text: \r\n`, `text: \n`.
pub(super) fn is_multiline_start(bytes: &[u8], start: usize) -> bool {
    bytes[start..].starts_with(b"text:")
        && (bytes[start..].starts_with(b"text:\r\n")
            || bytes[start..].starts_with(b"text:\n")
            || bytes[start..].starts_with(b"text: \r\n")
            || bytes[start..].starts_with(b"text: \n"))
}

/// Scan a multi-line `text:` literal. RFC 5228 §2.4.2 terminator
/// is a single `.` on its own line; `..` at the start of a line
/// is dot-stuffing and becomes a single dot. Returns the body
/// content and the index just past the terminator's LF.
pub(super) fn scan_multiline(bytes: &[u8], start: usize) -> Result<(String, usize), TokenizeError> {
    // skip "text:" and any trailing space/tab/CR, plus the LF
    let mut i = start + "text:".len();
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b'\r') {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'\n' {
        return Err(TokenizeError::UnterminatedMultiline(start));
    }
    i += 1; // past the LF
    let mut s = String::new();
    loop {
        if i >= bytes.len() {
            return Err(TokenizeError::UnterminatedMultiline(start));
        }
        // dot-stuffing terminator: ".\r\n" or ".\n" on its own line
        if bytes[i] == b'.'
            && (bytes[i..].starts_with(b".\r\n") || bytes[i..].starts_with(b".\n"))
        {
            let advance = if bytes[i..].starts_with(b".\r\n") { 3 } else { 2 };
            return Ok((s, i + advance));
        }
        // dot-stuffed line ("..") becomes a single dot
        if bytes[i] == b'.' && i + 1 < bytes.len() && bytes[i + 1] == b'.' {
            s.push('.');
            i += 2;
            continue;
        }
        s.push(bytes[i] as char);
        i += 1;
    }
}

fn utf8_char_len(first_byte: u8) -> usize {
    if first_byte < 0xC0 {
        1 // ASCII (< 0x80) and continuation bytes (0x80-0xBF) both
          // advance one byte — the latter shouldn't appear at the
          // start of a code point in valid UTF-8, but we stay
          // forgiving rather than panic.
    } else if first_byte < 0xE0 {
        2
    } else if first_byte < 0xF0 {
        3
    } else {
        4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_simple() {
        let (s, n) = scan_quoted(br#""hello""#, 0).unwrap();
        assert_eq!(s, "hello");
        assert_eq!(n, 7);
    }

    #[test]
    fn quoted_escaped_quote() {
        let (s, n) = scan_quoted(br#""he said \"hi\"""#, 0).unwrap();
        assert_eq!(s, r#"he said "hi""#);
        assert_eq!(n, 16);
    }

    #[test]
    fn quoted_unterminated() {
        let err = scan_quoted(b"\"oops", 0).unwrap_err();
        assert_eq!(err, TokenizeError::UnterminatedString(0));
    }

    #[test]
    fn multiline_simple() {
        let src = b"text:\nhello world\n.\n";
        let (s, n) = scan_multiline(src, 0).unwrap();
        assert_eq!(s, "hello world\n");
        assert_eq!(n, src.len());
    }

    #[test]
    fn multiline_dot_stuffed() {
        let src = b"text:\n..stuffed\n.\n";
        let (s, _) = scan_multiline(src, 0).unwrap();
        assert_eq!(s, ".stuffed\n");
    }

    #[test]
    fn multiline_unterminated() {
        let src = b"text:\nnever closed";
        let err = scan_multiline(src, 0).unwrap_err();
        assert_eq!(err, TokenizeError::UnterminatedMultiline(0));
    }
}
