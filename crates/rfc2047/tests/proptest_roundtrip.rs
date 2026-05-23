//! Property-based roundtrip tests for `encode` and `decode`.
//!
//! The contract: for every UTF-8 string `s`, `decode(encode(s).as_bytes()) == s`.
//! `proptest` generates thousands of random inputs each test run,
//! covering corners like literal `=?` sequences, control bytes, full
//! Unicode planes, mixed ASCII + non-ASCII.
//!
//! Complements the fuzz target — same property, but runs as part of
//! `cargo test` so it can't accidentally be skipped.

use mailrs_rfc2047::{decode, encode};
use proptest::prelude::*;

proptest! {
    /// Roundtrip: arbitrary UTF-8 → encode → decode → original.
    #[test]
    fn arbitrary_utf8_roundtrip(s in any::<String>()) {
        let encoded = encode(&s);
        let decoded = decode(encoded.as_bytes());
        prop_assert_eq!(decoded.as_ref(), &s);
    }

    /// Roundtrip with strings explicitly containing `=?` markers — the
    /// regression case caught by the fuzz target (see CHANGELOG 1.1.2).
    #[test]
    fn ascii_with_encoded_word_markers_roundtrip(s in "[a-zA-Z0-9= ?\\.\\-_]+") {
        let encoded = encode(&s);
        let decoded = decode(encoded.as_bytes());
        prop_assert_eq!(decoded.as_ref(), &s);
    }

    /// Roundtrip with Asian text — exercises ISO-2022-JP / UTF-8
    /// multibyte sequences in the encoded output.
    #[test]
    fn cjk_roundtrip(s in "[\u{4E00}-\u{9FFF}\u{3040}-\u{30FF}]+") {
        let encoded = encode(&s);
        let decoded = decode(encoded.as_bytes());
        prop_assert_eq!(decoded.as_ref(), &s);
    }

    /// Roundtrip with control bytes — bytes 0x00-0x1F (except CR/LF
    /// which are real header structure) survive encode/decode.
    #[test]
    fn control_bytes_roundtrip(s in "[\u{0001}-\u{0008}\u{000B}-\u{000C}\u{000E}-\u{001F}]+") {
        let encoded = encode(&s);
        let decoded = decode(encoded.as_bytes());
        prop_assert_eq!(decoded.as_ref(), &s);
    }
}
