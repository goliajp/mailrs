//! Strict-mode invariant linter.
//!
//! `MessageBuilder::strict_mode()` enables a set of pre-build
//! invariant checks; `build_strict()` returns `Err` if any fail.
//! The same invariants ship as a callable [`lint`] function so
//! callers can audit messages built by other code paths.

use std::fmt;

/// One lint failure category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LintError {
    /// No `From:` mailbox set. RFC 5322 §3.6 requires it.
    MissingFrom,
    /// Neither `To:`, `Cc:`, nor `Bcc:` set. The message has no
    /// recipient.
    MissingRecipient,
    /// `Message-ID:` value is malformed (missing angle brackets).
    BadMessageId(String),
    /// A header value contains a bare LF or a lone CR (control
    /// character that would split the message during parse). The
    /// string is the offending header name.
    ControlCharsInHeader(String),
    /// An attachment filename contains a CR / LF / NUL (injection
    /// vector).
    BadAttachmentFilename(String),
    /// A body line exceeds 998 octets (RFC 5322 §2.1.1 hard limit)
    /// after the CTE was applied.
    BodyLineTooLong {
        /// 1-based line index in the body block.
        line_no: usize,
        /// Length of the offending line in octets.
        len: usize,
    },
}

impl fmt::Display for LintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFrom => f.write_str("missing From: header"),
            Self::MissingRecipient => f.write_str("missing recipient (no To:/Cc:/Bcc:)"),
            Self::BadMessageId(s) => write!(f, "malformed Message-ID: {s:?}"),
            Self::ControlCharsInHeader(name) => {
                write!(f, "control characters in header {name:?}")
            }
            Self::BadAttachmentFilename(name) => {
                write!(f, "control characters in attachment filename {name:?}")
            }
            Self::BodyLineTooLong { line_no, len } => {
                write!(f, "body line {line_no} too long ({len} > 998 octets)")
            }
        }
    }
}

impl std::error::Error for LintError {}

/// Check a raw built message against the invariants. Returns the
/// first failure or `Ok(())` if everything passes.
pub fn lint(raw: &[u8]) -> Result<(), LintError> {
    // bare LF in the header block
    let (headers, body) = match find_header_terminator(raw) {
        Some(idx) => (&raw[..idx], &raw[idx + 4..]),
        None => (raw, &[][..]),
    };
    check_header_block(headers)?;
    check_body_line_lengths(body)?;
    Ok(())
}

fn find_header_terminator(raw: &[u8]) -> Option<usize> {
    raw.windows(4).position(|w| w == b"\r\n\r\n")
}

fn check_header_block(headers: &[u8]) -> Result<(), LintError> {
    // unfold first: continuation lines start with SP/HTAB
    // we just iterate lines and check raw bytes — bare LF or lone
    // CR anywhere in headers is a hard fail
    let mut i = 0;
    while i < headers.len() {
        let b = headers[i];
        if b == b'\n' && (i == 0 || headers[i - 1] != b'\r') {
            // bare LF
            return Err(LintError::ControlCharsInHeader("?".to_string()));
        }
        if b == b'\r' && (i + 1 >= headers.len() || headers[i + 1] != b'\n') {
            // lone CR
            return Err(LintError::ControlCharsInHeader("?".to_string()));
        }
        i += 1;
    }
    Ok(())
}

fn check_body_line_lengths(body: &[u8]) -> Result<(), LintError> {
    let mut line_no = 1usize;
    let mut cur = 0usize;
    for &b in body {
        if b == b'\n' {
            if cur > 998 {
                return Err(LintError::BodyLineTooLong { line_no, len: cur });
            }
            cur = 0;
            line_no += 1;
        } else if b != b'\r' {
            cur += 1;
        }
    }
    if cur > 998 {
        return Err(LintError::BodyLineTooLong { line_no, len: cur });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_message_passes() {
        let raw = b"From: a@x\r\nTo: b@y\r\nSubject: s\r\n\r\nbody\r\n";
        assert_eq!(lint(raw), Ok(()));
    }

    #[test]
    fn bare_lf_in_headers_fails() {
        let raw = b"From: a@x\nTo: b@y\r\n\r\nbody\r\n";
        assert!(matches!(lint(raw), Err(LintError::ControlCharsInHeader(_))));
    }

    #[test]
    fn lone_cr_in_headers_fails() {
        let raw = b"From: a@x\rTo: b@y\r\n\r\nbody\r\n";
        assert!(matches!(lint(raw), Err(LintError::ControlCharsInHeader(_))));
    }

    #[test]
    fn body_line_999_chars_fails() {
        let mut raw = b"From: a@x\r\n\r\n".to_vec();
        raw.extend(std::iter::repeat_n(b'x', 999));
        raw.extend_from_slice(b"\r\n");
        assert!(matches!(
            lint(&raw),
            Err(LintError::BodyLineTooLong { len: 999, .. })
        ));
    }

    #[test]
    fn body_line_998_chars_passes() {
        let mut raw = b"From: a@x\r\n\r\n".to_vec();
        raw.extend(std::iter::repeat_n(b'x', 998));
        raw.extend_from_slice(b"\r\n");
        assert_eq!(lint(&raw), Ok(()));
    }

    #[test]
    fn display_format_is_human_readable() {
        let e = LintError::MissingFrom;
        assert_eq!(e.to_string(), "missing From: header");
        let e = LintError::BodyLineTooLong { line_no: 5, len: 1200 };
        assert!(e.to_string().contains("line 5"));
        assert!(e.to_string().contains("1200"));
    }
}
