//! Header iteration types — emitted by [`Message::headers`](crate::Message::headers).

/// One header line in a message, returned by [`HeaderIter`].
///
/// Both name and value borrow from the original message bytes — no
/// allocation is performed on the parse side. The value has had its
/// leading whitespace (one space typically, sometimes a tab) trimmed
/// but otherwise carries the wire-format bytes verbatim, including any
/// CRLF + WSP "folded" continuation lines joined back into one slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header<'a> {
    /// Field name — case-insensitive per RFC 5322 §3.6.8. The crate
    /// preserves the original case; comparators should fold.
    pub name: &'a str,
    /// Field value bytes, as they appear after the colon. Leading
    /// whitespace trimmed (just the one space/tab after the colon).
    /// For folded headers (continuation lines starting with WSP), the
    /// CRLF + WSP sequences are kept in the slice — this is the raw
    /// wire form, the caller decides whether to unfold.
    pub value: &'a [u8],
}

impl<'a> Header<'a> {
    /// Get the value as a `&str` if it's valid UTF-8. RFC 5322 §2.2
    /// says fields are 7-bit ASCII; RFC 6532 extends to UTF-8. Almost
    /// every real message's headers fit one of those. Returns `None`
    /// for the rare malformed case.
    pub fn value_str(&self) -> Option<&'a str> {
        std::str::from_utf8(self.value).ok()
    }
}

/// Iterator over all headers in a message, in the order they appear.
///
/// Returned by [`Message::headers`](crate::Message::headers). Stops at
/// the empty line that separates headers from the body (or at EOF if
/// the message has no body).
pub struct HeaderIter<'a> {
    pub(crate) bytes: &'a [u8],
    pub(crate) cursor: usize,
}

impl<'a> Iterator for HeaderIter<'a> {
    type Item = Header<'a>;

    fn next(&mut self) -> Option<Header<'a>> {
        let start = self.cursor;
        if start >= self.bytes.len() {
            return None;
        }

        // Find end of this logical header line (handles RFC 5322 §3.2.2
        // line folding: continuation lines starting with WSP belong
        // to this header).
        let (line_end, after_crlf) = match find_unfolded_line_end(self.bytes, start) {
            Some(pair) => pair,
            None => {
                // Trailing partial header at EOF — emit it.
                self.cursor = self.bytes.len();
                let line = &self.bytes[start..];
                if line.is_empty() {
                    return None;
                }
                return parse_header_line(self.bytes, start, self.bytes.len());
            }
        };

        // Empty line (CRLF or LF only) marks the body boundary.
        if line_end == start {
            self.cursor = self.bytes.len(); // stop after this point
            return None;
        }

        self.cursor = after_crlf;
        parse_header_line(self.bytes, start, line_end)
    }
}

/// Find the end of the logical header line that starts at `start`.
///
/// Returns `Some((line_end, after_crlf))` where `line_end` is the byte
/// offset of the terminating CRLF/LF (so `&bytes[start..line_end]` is
/// the header line content) and `after_crlf` is the offset of the
/// next line's start. Handles folding: a CRLF/LF followed by WSP is
/// NOT a line terminator (it's a continuation).
///
/// Returns `None` if no terminator is found before EOF.
pub(crate) fn find_unfolded_line_end(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
    let mut i = start;
    while i < bytes.len() {
        // Locate the next LF (covers both \n and \r\n line endings).
        let lf = bytes[i..].iter().position(|&b| b == b'\n');
        let lf_abs = match lf {
            Some(off) => i + off,
            None => return None,
        };

        // line content end (strip trailing \r if present)
        let mut content_end = lf_abs;
        if content_end > start && bytes[content_end - 1] == b'\r' {
            content_end -= 1;
        }

        let next = lf_abs + 1;
        // Is the next line a continuation? (starts with SP or HTAB)
        if next < bytes.len() && (bytes[next] == b' ' || bytes[next] == b'\t') {
            // Continuation — keep scanning; the LF we found is part of
            // this logical line.
            i = next;
            continue;
        }

        return Some((content_end, next));
    }
    None
}

/// Parse one header line `bytes[start..line_end]` into a `Header`.
///
/// Returns `None` if the line lacks a colon (malformed) — RFC 5322 says
/// such a line terminates the header block, but we already detected
/// empty-line termination above; a malformed line here is just skipped.
fn parse_header_line(bytes: &[u8], start: usize, line_end: usize) -> Option<Header<'_>> {
    let line = &bytes[start..line_end];
    let colon = line.iter().position(|&b| b == b':')?;
    // Name is line[..colon]. RFC 5322 §3.6.8: name is printable ASCII
    // excluding colon; we don't validate (real-world messages
    // sometimes have spaces or other anomalies — let downstream decide).
    let name = std::str::from_utf8(&line[..colon]).ok()?;
    // Skip the colon + at most one optional WSP after it.
    let mut value_start_local = colon + 1;
    if value_start_local < line.len()
        && (line[value_start_local] == b' ' || line[value_start_local] == b'\t')
    {
        value_start_local += 1;
    }
    Some(Header {
        name,
        value: &line[value_start_local..],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfolded_line_end_handles_lf() {
        let bytes = b"Subject: hi\nFrom: x\n\nbody";
        let (end, after) = find_unfolded_line_end(bytes, 0).unwrap();
        assert_eq!(end, 11); // "Subject: hi"
        assert_eq!(after, 12); // after \n
    }

    #[test]
    fn unfolded_line_end_handles_crlf() {
        let bytes = b"Subject: hi\r\nFrom: x\r\n";
        let (end, after) = find_unfolded_line_end(bytes, 0).unwrap();
        assert_eq!(end, 11); // content ends before \r
        assert_eq!(after, 13); // after \r\n
    }

    #[test]
    fn folded_line_keeps_both_lines_in_one_header() {
        //                0         1         2
        //                0123456789012345678901234
        let bytes = b"Subject: first\r\n second\r\nFrom: x\r\n";
        // Continuation line " second" is part of Subject. The unfolded
        // line ends at the second \r\n's \r (offset 23), and after_crlf
        // points past the \r\n (offset 25).
        let (end, after) = find_unfolded_line_end(bytes, 0).unwrap();
        // line_end points at the \r of the terminating \r\n (after
        // "Subject: first\r\n second"). bytes[..end] spans everything
        // up to but not including that \r.
        assert!(bytes[..end].ends_with(b"second"));
        // after_crlf is the next-line start: just past the \r\n.
        assert_eq!(after, 25);
        // and the next line, scanned from there, is "From: x"
        let (end2, _) = find_unfolded_line_end(bytes, after).unwrap();
        assert_eq!(&bytes[after..end2], b"From: x");
    }

    #[test]
    fn parse_simple_header() {
        let bytes = b"Subject: hello\r\n";
        let h = parse_header_line(bytes, 0, 14).unwrap();
        assert_eq!(h.name, "Subject");
        assert_eq!(h.value, b"hello");
    }

    #[test]
    fn parse_header_with_tab_after_colon() {
        let bytes = b"X-Custom:\thi\r\n";
        let h = parse_header_line(bytes, 0, 12).unwrap();
        assert_eq!(h.name, "X-Custom");
        assert_eq!(h.value, b"hi");
    }

    #[test]
    fn parse_header_no_space_after_colon() {
        let bytes = b"X:hi\r\n";
        let h = parse_header_line(bytes, 0, 4).unwrap();
        assert_eq!(h.name, "X");
        assert_eq!(h.value, b"hi");
    }

    #[test]
    fn parse_header_without_colon_returns_none() {
        let bytes = b"malformed\r\n";
        assert!(parse_header_line(bytes, 0, 9).is_none());
    }
}
