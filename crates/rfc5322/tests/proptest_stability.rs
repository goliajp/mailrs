//! Property-based stability tests for `Message::new` + header lookup.
//!
//! Contract: the byte-level parser must never panic, regardless of
//! input. All public methods on `Message` should return sensibly
//! (empty / None) on garbage input rather than crashing.
//!
//! Complements unit tests + the fuzz target — same property, but runs
//! as part of `cargo test` so it can't accidentally be skipped.

use mailrs_rfc5322::Message;
use proptest::prelude::*;

proptest! {
    /// Arbitrary bytes never panic the parser.
    #[test]
    fn arbitrary_bytes_dont_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let msg = Message::new(&bytes);
        // exercise every public method on the message
        let _ = msg.raw();
        let _ = msg.header("From");
        let _ = msg.header("Subject");
        let _ = msg.header_str("Date");
        let _ = msg.body_offset();
        let _ = msg.body();
        for h in msg.headers() {
            let _ = h.name;
            let _ = h.value;
            let _ = h.value_str();
        }
    }

    /// `header()` is case-insensitive on the lookup key.
    #[test]
    fn header_lookup_case_insensitive(
        body in any::<String>(),
        name_lower in "[a-z][a-z0-9-]{0,30}",
    ) {
        let raw = format!("{}: foo\r\n\r\n{}", name_lower, body);
        let msg = Message::new(raw.as_bytes());
        let lower_hit = msg.header(&name_lower).is_some();
        let upper_hit = msg.header(&name_lower.to_ascii_uppercase()).is_some();
        let mixed = {
            let chars: String = name_lower
                .chars()
                .enumerate()
                .map(|(i, c)| if i % 2 == 0 { c.to_ascii_uppercase() } else { c })
                .collect();
            msg.header(&chars).is_some()
        };
        prop_assert!(lower_hit == upper_hit && upper_hit == mixed,
            "header({name_lower:?}) lookup not case-insensitive: lower={lower_hit} upper={upper_hit} mixed={mixed}");
    }

    /// `body()` returns either None (no terminator) or the slice strictly
    /// after the `\r\n\r\n` / `\n\n` boundary. Never panics.
    #[test]
    fn body_offset_invariant(bytes in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let msg = Message::new(&bytes);
        match (msg.body_offset(), msg.body()) {
            (Some(off), Some(body)) => {
                prop_assert!(off <= bytes.len());
                prop_assert_eq!(body, &bytes[off..]);
            }
            (None, None) => {}
            other => prop_assert!(false, "inconsistent body offset/body: {:?}", other),
        }
    }
}
