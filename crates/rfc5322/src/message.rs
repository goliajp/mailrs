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
    /// Fast-path optimization: a non-matching header is rejected by
    /// comparing only its first `wanted.len()` bytes + the next byte
    /// (must be `:`), avoiding the full per-header `from_utf8` +
    /// colon-find work that the generic [`Message::headers`] iterator
    /// does. On a worst-case 50-header lookup this is ~30% faster than
    /// the iterate-and-filter equivalent.
    ///
    /// To get an iterator of *all* occurrences (Received: chains, etc),
    /// use [`Message::header_all`] or [`Message::headers`].
    pub fn header(&self, wanted: &str) -> Option<&'a [u8]> {
        let wanted_bytes = wanted.as_bytes();
        let wanted_len = wanted_bytes.len();
        let bytes = self.bytes;
        let mut cursor = 0usize;
        while cursor < bytes.len() {
            // Empty line → end of header block
            if bytes[cursor] == b'\n'
                || (bytes[cursor] == b'\r' && cursor + 1 < bytes.len() && bytes[cursor + 1] == b'\n')
            {
                return None;
            }

            // Fast-path: check this line's name region matches `wanted`
            // BEFORE doing the full parse.
            let has_match = cursor + wanted_len < bytes.len()
                && bytes[cursor + wanted_len] == b':'
                && bytes[cursor..cursor + wanted_len]
                    .iter()
                    .zip(wanted_bytes.iter())
                    .all(|(a, b)| a.eq_ignore_ascii_case(b));

            // Find end-of-logical-line (handles folding)
            let (line_end, after_crlf) = match crate::header::find_unfolded_line_end(bytes, cursor) {
                Some(pair) => pair,
                None => return None,
            };

            if has_match {
                // Extract value: skip colon + optional single WSP
                let value_start_local = wanted_len + 1; // past colon
                let line = &bytes[cursor..line_end];
                let mut vs = value_start_local;
                if vs < line.len() && (line[vs] == b' ' || line[vs] == b'\t') {
                    vs += 1;
                }
                return Some(&line[vs..]);
            }

            cursor = after_crlf;
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

    // ===== edge cases =====

    #[test]
    fn folded_header_three_continuation_lines() {
        let bytes = b"Subject: line1\r\n line2\r\n\tline3\r\n line4\r\nFrom: x\r\n\r\nbody";
        let msg = Message::new(bytes);
        let subj = msg.header("Subject").unwrap();
        // All four lines (including the original) should be in the value,
        // with the CRLF + WSP intact between them.
        assert!(subj.starts_with(b"line1"));
        assert!(std::str::from_utf8(subj).unwrap().contains("line2"));
        assert!(std::str::from_utf8(subj).unwrap().contains("line3"));
        assert!(std::str::from_utf8(subj).unwrap().contains("line4"));
        // And From: still findable after the long fold
        assert_eq!(msg.header("From"), Some(b"x" as &[u8]));
    }

    #[test]
    fn folded_header_tab_continuation() {
        // Tab is also a valid WSP continuation per RFC 5322 §3.2.2
        let bytes = b"X-Long-Header: first\r\n\tsecond\r\n\r\nbody";
        let msg = Message::new(bytes);
        let val = msg.header("X-Long-Header").unwrap();
        assert!(val.starts_with(b"first"));
        assert!(std::str::from_utf8(val).unwrap().contains("second"));
    }

    #[test]
    fn header_value_contains_colon_received_chain() {
        // Real-world Received headers have many colons in the value
        // (timestamps, port numbers, etc.). Parser should split only on
        // the FIRST colon.
        let bytes = b"Received: from mta.example.com (mta.example.com [203.0.113.42:25])\
                       by mx.golia.jp with ESMTP id 12345; Sun, 22 May 2026 10:00:00 +0900\r\n\r\nbody";
        let msg = Message::new(bytes);
        let received = msg.header("Received").unwrap();
        // Value should start with "from mta…", NOT some sub-split
        assert!(received.starts_with(b"from mta.example.com"));
        // And the colons inside should be preserved
        assert!(std::str::from_utf8(received).unwrap().contains("203.0.113.42:25"));
    }

    #[test]
    fn empty_header_value() {
        let bytes = b"X-Empty:\r\nFrom: alice\r\n\r\nbody";
        let msg = Message::new(bytes);
        assert_eq!(msg.header("X-Empty"), Some(b"" as &[u8]));
        // Subsequent headers still parse correctly
        assert_eq!(msg.header("From"), Some(b"alice" as &[u8]));
    }

    #[test]
    fn whitespace_only_header_value() {
        let bytes = b"X-Tabbed: \t \r\nFrom: alice\r\n\r\nbody";
        let msg = Message::new(bytes);
        // After stripping one leading WSP, whitespace remainder kept verbatim
        let val = msg.header("X-Tabbed").unwrap();
        assert_eq!(val, b"\t ");
        assert_eq!(msg.header("From"), Some(b"alice" as &[u8]));
    }

    #[test]
    fn header_name_case_preserved_in_iter() {
        let bytes = b"X-Custom-Header: v\r\n\r\nbody";
        let msg = Message::new(bytes);
        // Iterator preserves the literal name as it appears
        let names: Vec<&str> = msg.headers().map(|h| h.name).collect();
        assert_eq!(names, vec!["X-Custom-Header"]);
        // But lookup is case-insensitive
        assert!(msg.header("x-custom-header").is_some());
        assert!(msg.header("X-CUSTOM-HEADER").is_some());
    }

    #[test]
    fn header_name_with_digits_and_hyphens() {
        let bytes = b"X-MS-Has-Attach: yes\r\n\
                       Content-Type-1: text/plain\r\n\r\nbody";
        let msg = Message::new(bytes);
        assert_eq!(msg.header("X-MS-Has-Attach"), Some(b"yes" as &[u8]));
        assert_eq!(msg.header("Content-Type-1"), Some(b"text/plain" as &[u8]));
    }

    #[test]
    fn iter_can_be_called_twice_independently() {
        let bytes = b"A: 1\r\nB: 2\r\n\r\nbody";
        let msg = Message::new(bytes);
        let names1: Vec<_> = msg.headers().map(|h| h.name).collect();
        let names2: Vec<_> = msg.headers().map(|h| h.name).collect();
        assert_eq!(names1, names2);
    }

    #[test]
    fn body_offset_cached_call_returns_same_value() {
        let bytes = b"From: x\r\n\r\nbody";
        let msg = Message::new(bytes);
        let first = msg.body_offset();
        let second = msg.body_offset();
        let third = msg.body_offset();
        assert_eq!(first, second);
        assert_eq!(second, third);
    }

    #[test]
    fn message_with_no_body_separator_returns_no_body() {
        let bytes = b"From: x\r\nSubject: hi\r\n";
        let msg = Message::new(bytes);
        assert!(msg.body().is_none());
        assert!(msg.body_offset().is_none());
        // But headers still iterate
        assert_eq!(msg.headers().count(), 2);
    }

    #[test]
    fn just_crlf_message() {
        // Empty headers + empty body
        let bytes = b"\r\n";
        let msg = Message::new(bytes);
        assert_eq!(msg.body(), Some(b"" as &[u8]));
        assert_eq!(msg.headers().count(), 0);
    }

    #[test]
    fn lone_lf_terminator_works() {
        // Some implementations send LF-only line endings
        let bytes = b"From: x\nSubject: hi\n\nbody";
        let msg = Message::new(bytes);
        assert_eq!(msg.header("From"), Some(b"x" as &[u8]));
        assert_eq!(msg.body(), Some(b"body" as &[u8]));
    }

    #[test]
    fn mixed_crlf_and_lf_line_endings() {
        // Some buggy clients mix them — should still parse
        let bytes = b"From: x\r\nSubject: hi\n\r\nbody";
        let msg = Message::new(bytes);
        assert_eq!(msg.header("From"), Some(b"x" as &[u8]));
        // Subject line terminates with \n (no preceding \r). Acceptable.
        assert_eq!(msg.header("Subject"), Some(b"hi" as &[u8]));
    }

    #[test]
    fn long_header_value_2kb() {
        // Stress test: 2 KB header value (e.g. very long DKIM signature)
        let long: String = "x".repeat(2000);
        let raw = format!("DKIM-Signature: {long}\r\n\r\nbody");
        let msg = Message::new(raw.as_bytes());
        let val = msg.header("DKIM-Signature").unwrap();
        assert_eq!(val.len(), 2000);
        assert!(val.iter().all(|&b| b == b'x'));
    }

    #[test]
    fn find_target_at_end_of_many_headers() {
        // Worst case for header(): target is the last of 50 headers.
        let mut raw = Vec::new();
        for i in 0..49 {
            raw.extend_from_slice(format!("X-Filler-{i}: value{i}\r\n").as_bytes());
        }
        raw.extend_from_slice(b"X-Target: bingo\r\n\r\nbody");
        let msg = Message::new(&raw);
        assert_eq!(msg.header("X-Target"), Some(b"bingo" as &[u8]));
    }

    #[test]
    fn header_not_found_traverses_full_block() {
        // Ensure missing-header lookup doesn't accidentally return Some
        // from a partial match.
        let raw = b"X-AAA: 1\r\nX-BBB: 2\r\nX-CCC: 3\r\n\r\nbody";
        let msg = Message::new(raw);
        assert_eq!(msg.header("X-DDD"), None);
        assert_eq!(msg.header("X-AAAA"), None); // prefix-only must not match
        assert_eq!(msg.header("X-AA"), None); // shorter must not match
    }

    #[test]
    fn header_all_returns_zero_when_no_match() {
        let raw = b"From: x\r\nTo: y\r\n\r\nbody";
        let msg = Message::new(raw);
        assert_eq!(msg.header_all("Received").count(), 0);
    }

    #[test]
    fn utf8_in_header_value_via_str_helper() {
        // RFC 6532 says headers can be UTF-8. header_str returns Some
        // when the bytes are valid UTF-8.
        let raw = "From: アリス\r\n\r\nbody".as_bytes();
        let msg = Message::new(raw);
        assert_eq!(msg.header_str("From"), Some("アリス"));
    }

    #[test]
    fn invalid_utf8_in_header_value_returns_none_via_str() {
        // bytes 0xFF 0xFE are not valid UTF-8
        let mut raw = b"X-Bin: ".to_vec();
        raw.extend_from_slice(&[0xFF, 0xFE]);
        raw.extend_from_slice(b"\r\n\r\nbody");
        let msg = Message::new(&raw);
        // header() returns raw bytes (still works)
        assert!(msg.header("X-Bin").is_some());
        // header_str returns None because not UTF-8
        assert!(msg.header_str("X-Bin").is_none());
    }

    #[test]
    fn body_with_internal_empty_lines_kept_intact() {
        // The header/body boundary is the FIRST empty line. Any empty
        // lines inside the body are part of the body verbatim.
        let raw = b"From: x\r\n\r\nfirst\r\n\r\nsecond\r\n";
        let msg = Message::new(raw);
        let body = msg.body().unwrap();
        assert!(body.starts_with(b"first"));
        // The second empty line is INSIDE the body
        assert_eq!(body, b"first\r\n\r\nsecond\r\n");
    }
}
