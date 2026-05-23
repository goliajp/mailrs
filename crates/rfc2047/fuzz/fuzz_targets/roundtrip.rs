#![no_main]
//! Property: `encode` then `decode` returns the original UTF-8 input.
//! Useful for catching subtle Q-encoding / B-encoding corner cases.

use libfuzzer_sys::fuzz_target;
use mailrs_rfc2047::{decode, encode};

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let encoded = encode(s);
        let decoded = decode(encoded.as_bytes());
        // Sanity: roundtrip preserves content; we don't assert exact equality
        // because encode may legitimately rewrite the form, but the decoded
        // value must equal the original input.
        assert_eq!(decoded.as_ref(), s, "roundtrip mismatch");
    }
});
