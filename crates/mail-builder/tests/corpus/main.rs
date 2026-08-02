//! ckpt 2.1 — RFC test corpus.
//!
//! 30+ structured scenarios covering the MIME shapes mailrs
//! production paths actually emit, plus the RFC examples (2046,
//! 2047, 2231, 3464, 6376, 7489) the builder claims to support.
//! Each scenario builds via `MessageBuilder`, parses via
//! `mailrs-rfc5322` + `mailrs-mime`, and asserts structural
//! invariants. The goal isn't byte-identity to any external
//! source — it's that the builder never emits something a
//! conforming parser can't recover the structure from.

use mailrs_rfc5322::Message;

pub(crate) fn fixed_date() -> &'static str {
    "Wed, 27 May 2026 12:00:00 +0000"
}

/// Common assertion: `msg` parses cleanly as RFC 5322 and the
/// stated header subset matches.
pub(crate) fn assert_parses_with_headers(msg: &[u8], expected: &[(&str, &str)]) {
    let parsed = Message::new(msg);
    assert!(
        parsed.body_offset().is_some(),
        "message has no body separator"
    );
    for (name, want_contains) in expected {
        let got = parsed.header_str(name).unwrap_or_default();
        let unfold = got.replace("\r\n ", " ").replace("\r\n\t", " ");
        assert!(
            unfold.contains(want_contains),
            "header {name}: want substring {want_contains:?}, got {unfold:?}",
        );
    }
}

pub(crate) fn assert_mime_multipart(msg: &[u8], expected_type: &str, expected_children: usize) {
    let p = mailrs_mime::part::parse(msg);
    assert!(
        p.content_type.is_multipart(),
        "expected multipart, got {}",
        p.content_type.mime_type()
    );
    assert_eq!(
        p.content_type.mime_type(),
        expected_type,
        "wrong outer multipart subtype",
    );
    assert_eq!(p.children.len(), expected_children, "wrong child count");
}

mod bodies;
mod headers;
mod multipart;
mod rfc;
