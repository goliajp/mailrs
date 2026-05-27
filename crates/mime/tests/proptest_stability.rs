//! Property-based stability tests for the MIME parser.
//!
//! Contract: `mailrs_mime::parse` must never panic, regardless of
//! input bytes. The walk / body_text / find_by_content_type /
//! attachment iteration paths must also be panic-safe on the
//! returned `Part` tree.
//!
//! Complements the existing unit tests + the fuzz target — same
//! property, but runs as part of `cargo test`.

use mailrs_mime::parse;
use proptest::prelude::*;

proptest! {
    /// Arbitrary bytes never panic the parser or downstream walk.
    #[test]
    fn arbitrary_bytes_dont_panic(bytes in proptest::collection::vec(any::<u8>(), 0..8192)) {
        let part = parse(&bytes);
        // exercise the walker (depth-first whole tree)
        for child in part.walk() {
            let _ = child.attachment_filename();
            let _ = child.is_attachment();
        }
        // text extraction
        let _ = part.body_text();
        // content-type lookup
        let _ = part.find_by_content_type("text/plain");
        let _ = part.find_by_content_type("application/octet-stream");
        // attachment iteration
        for att in part.attachments() {
            let _ = att.attachment_filename();
        }
    }

    /// A minimal text/plain body produces a non-empty body_text result.
    #[test]
    fn text_plain_body_extracts(body in "[a-zA-Z0-9 ]{1,200}") {
        let raw = format!("Content-Type: text/plain\r\n\r\n{}", body);
        let part = parse(raw.as_bytes());
        let text = part.body_text();
        prop_assert!(text.is_some(), "text/plain body must extract");
        let extracted = text.unwrap();
        prop_assert_eq!(extracted.trim_end(), body.trim_end());
    }

    /// `walk` always yields at least one node (the root itself).
    #[test]
    fn walk_includes_root(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let part = parse(&bytes);
        let count = part.walk().count();
        prop_assert!(count >= 1, "walker must yield at least the root");
    }
}
