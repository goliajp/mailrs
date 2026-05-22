//! [`Message`] — the top-level type. Holds a `&[u8]` slice of the raw
//! message and provides lazy header / body access.

use crate::header::{find_unfolded_line_end, Header, HeaderIter};

/// A parsed-on-demand RFC 5322 message backed by the caller's bytes.
///
/// Construction is `O(1)` — `Message::new(bytes)` just stores the
/// slice. Header lookup is `O(header-region-size)`, not `O(message-size)`:
/// the scanner stops at the empty-line boundary, so finding "Subject"
/// in a 5 MB attachment-bearing message takes the same time as finding
/// it in a 5 KB one (modulo cache effects).
///
/// Body access (`body()` / `body_offset()`) caches the offset on first
/// call, so repeated body accesses are `O(1)`.
///
/// Zero allocation in the hot path: header values come back as
/// `&[u8]` borrowing from the input. The caller decides whether to
/// allocate.
#[derive(Debug, Clone)]
pub struct Message<'a> {
    bytes: &'a [u8],
    /// Cached offset of the start of the body (the byte after the
    /// empty CRLF/LF that terminates the header block). `None` means
    /// "not yet computed"; `Some(usize::MAX)` means "no body in this
    /// message".
    body_offset: core::cell::Cell<Option<usize>>,
}

impl<'a> Message<'a> {
    /// Wrap a raw message byte slice. No parsing is performed yet.
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            body_offset: core::cell::Cell::new(None),
        }
    }

    /// The original message bytes the parser was constructed with.
    pub fn raw(&self) -> &'a [u8] {
        self.bytes
    }

    /// Look up the first header with name `wanted` (case-insensitive
    /// per RFC 5322 §3.6.8). Returns the value bytes, or `None` if no
    /// such header exists.
    ///
    /// This is `O(header-region-size)` — the scanner stops at the
    /// empty line that separates headers from the body, so it never
    /// reads the body even on multi-MB attachments.
    ///
    /// To get an iterator of *all* occurrences (Received: chains, etc),
    /// use [`Message::headers`] and filter manually.
    pub fn header(&self, wanted: &str) -> Option<&'a [u8]> {
        let wanted_bytes = wanted.as_bytes();
        for h in self.headers() {
            if eq_ignore_ascii_case(h.name.as_bytes(), wanted_bytes) {
                return Some(h.value);
            }
        }
        None
    }

    /// Look up the first header with name `wanted` (case-insensitive)
    /// as a `&str`. Convenience wrapper over [`Message::header`] +
    /// UTF-8 check.
    ///
    /// Returns `None` for missing OR for non-UTF-8 header values
    /// (RFC 6532 says they should be UTF-8; pre-6532 they're 7-bit
    /// ASCII; real malformed messages exist).
    pub fn header_str(&self, wanted: &str) -> Option<&'a str> {
        let value = self.header(wanted)?;
        std::str::from_utf8(value).ok()
    }

    /// Iterate over every header in document order. Stops at the
    /// empty line that separates headers from the body.
    pub fn headers(&self) -> HeaderIter<'a> {
        HeaderIter {
            bytes: self.bytes,
            cursor: 0,
        }
    }

    /// Iterate over every occurrence of a given header name
    /// (case-insensitive). For headers that can appear multiple times
    /// (Received, Authentication-Results, …), use this instead of
    /// `header()`.
    pub fn header_all(&self, wanted: &'a str) -> impl Iterator<Item = Header<'a>> + 'a {
        let wanted_bytes = wanted.as_bytes();
        self.headers().filter(move |h| {
            eq_ignore_ascii_case(h.name.as_bytes(), wanted_bytes)
        })
    }

    /// Locate the offset of the empty-line terminator separating
    /// headers from body. Memoized on the message — first call is
    /// `O(header-region-size)`, subsequent calls are `O(1)`.
    ///
    /// Returns `Some(offset)` where `bytes[offset]` is the first byte
    /// of the body. Returns `None` if the message has no body
    /// separator (no empty line found — entirely a header block).
    pub fn body_offset(&self) -> Option<usize> {
        if let Some(cached) = self.body_offset.get() {
            return (cached != usize::MAX).then_some(cached);
        }

        // Scan headers to find the empty line.
        let mut cursor = 0;
        loop {
            if cursor >= self.bytes.len() {
                self.body_offset.set(Some(usize::MAX));
                return None;
            }

            // empty-line check: this position starts with \r\n or \n
            if self.bytes[cursor] == b'\n' {
                self.body_offset.set(Some(cursor + 1));
                return Some(cursor + 1);
            }
            if self.bytes[cursor] == b'\r'
                && cursor + 1 < self.bytes.len()
                && self.bytes[cursor + 1] == b'\n'
            {
                self.body_offset.set(Some(cursor + 2));
                return Some(cursor + 2);
            }

            // walk past this header line (handle folding)
            match find_unfolded_line_end(self.bytes, cursor) {
                Some((_, after)) => cursor = after,
                None => {
                    self.body_offset.set(Some(usize::MAX));
                    return None;
                }
            }
        }
    }

    /// The message body — bytes after the empty-line CRLF/LF that
    /// terminates the header block. Returns `None` if no header
    /// terminator was found.
    pub fn body(&self) -> Option<&'a [u8]> {
        let offset = self.body_offset()?;
        Some(&self.bytes[offset..])
    }
}

/// ASCII case-insensitive comparison. RFC 5322 header names are
/// case-insensitive ASCII. We don't call `String::to_lowercase` because
/// it allocates; the std slice method does the same byte-walk.
#[inline]
fn eq_ignore_ascii_case(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIMPLE: &[u8] = b"\
From: alice@example.com\r\n\
To: bob@example.com\r\n\
Subject: hi\r\n\
\r\n\
Hello, world!\r\n";

    #[test]
    fn raw_round_trips_bytes() {
        let msg = Message::new(SIMPLE);
        assert_eq!(msg.raw(), SIMPLE);
    }

    #[test]
    fn header_finds_first_occurrence() {
        let msg = Message::new(SIMPLE);
        assert_eq!(msg.header("Subject"), Some(b"hi" as &[u8]));
        assert_eq!(msg.header("From"), Some(b"alice@example.com" as &[u8]));
    }

    #[test]
    fn header_is_case_insensitive() {
        let msg = Message::new(SIMPLE);
        assert_eq!(msg.header("subject"), Some(b"hi" as &[u8]));
        assert_eq!(msg.header("SUBJECT"), Some(b"hi" as &[u8]));
        assert_eq!(msg.header("SuBjEcT"), Some(b"hi" as &[u8]));
    }

    #[test]
    fn header_str_returns_utf8() {
        let msg = Message::new(SIMPLE);
        assert_eq!(msg.header_str("Subject"), Some("hi"));
    }

    #[test]
    fn header_missing_returns_none() {
        let msg = Message::new(SIMPLE);
        assert_eq!(msg.header("X-Missing"), None);
    }

    #[test]
    fn body_extracts_correct_bytes() {
        let msg = Message::new(SIMPLE);
        assert_eq!(msg.body(), Some(b"Hello, world!\r\n" as &[u8]));
    }

    #[test]
    fn body_offset_is_memoized() {
        let msg = Message::new(SIMPLE);
        let first = msg.body_offset();
        let second = msg.body_offset();
        assert_eq!(first, second);
        assert!(first.is_some());
    }

    #[test]
    fn lf_only_line_endings_work() {
        let bytes = b"Subject: hi\nFrom: x\n\nbody";
        let msg = Message::new(bytes);
        assert_eq!(msg.header("Subject"), Some(b"hi" as &[u8]));
        assert_eq!(msg.header("From"), Some(b"x" as &[u8]));
        assert_eq!(msg.body(), Some(b"body" as &[u8]));
    }

    #[test]
    fn folded_header_value_includes_continuation() {
        let bytes = b"Subject: first\r\n second\r\nFrom: x\r\n\r\nbody";
        let msg = Message::new(bytes);
        let subj = msg.header("Subject").unwrap();
        // The raw value carries the CRLF + WSP unfolding the caller can
        // resolve. The "first" comes before the CRLF, "second" after.
        // RFC 5322 §3.2.2: WSP after CRLF means continuation.
        assert!(subj.starts_with(b"first"));
        // And the next header is still findable after folding.
        assert_eq!(msg.header("From"), Some(b"x" as &[u8]));
    }

    #[test]
    fn header_all_finds_multiple_received_chains() {
        let bytes = b"\
Received: from a.example.com\r\n\
Received: from b.example.com\r\n\
Received: from c.example.com\r\n\
\r\n\
body";
        let msg = Message::new(bytes);
        let received: Vec<_> = msg.header_all("Received").collect();
        assert_eq!(received.len(), 3);
        assert!(received[0].value.starts_with(b"from a"));
        assert!(received[1].value.starts_with(b"from b"));
        assert!(received[2].value.starts_with(b"from c"));
    }

    #[test]
    fn no_body_returns_none() {
        let bytes = b"Subject: header-only\r\n";
        let msg = Message::new(bytes);
        assert_eq!(msg.body(), None);
    }

    #[test]
    fn headers_iterator_returns_all_in_order() {
        let msg = Message::new(SIMPLE);
        let names: Vec<&str> = msg.headers().map(|h| h.name).collect();
        assert_eq!(names, vec!["From", "To", "Subject"]);
    }

    #[test]
    fn header_value_with_no_wsp_after_colon() {
        let bytes = b"X-Custom:value\r\n\r\n";
        let msg = Message::new(bytes);
        assert_eq!(msg.header("X-Custom"), Some(b"value" as &[u8]));
    }

    #[test]
    fn header_value_with_tab_after_colon() {
        let bytes = b"X-Custom:\tvalue\r\n\r\n";
        let msg = Message::new(bytes);
        assert_eq!(msg.header("X-Custom"), Some(b"value" as &[u8]));
    }

    #[test]
    fn empty_message_returns_no_body() {
        let msg = Message::new(b"");
        assert_eq!(msg.body(), None);
        assert_eq!(msg.headers().count(), 0);
    }

    #[test]
    fn body_only_message_works() {
        // header-then-empty-line-then-body is the canonical shape;
        // an empty leading CRLF means "no headers, all body".
        let bytes = b"\r\nbody here";
        let msg = Message::new(bytes);
        assert_eq!(msg.body(), Some(b"body here" as &[u8]));
        assert_eq!(msg.headers().count(), 0);
    }
}
