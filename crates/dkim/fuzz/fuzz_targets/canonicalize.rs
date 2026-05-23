#![no_main]
//! Fuzz canonicalize_body + canonicalize_header. Goal: no panic on
//! any byte sequence; outputs are bounded by inputs.

use libfuzzer_sys::fuzz_target;
use mailrs_dkim::canon::{canonicalize_body, canonicalize_header};
use mailrs_dkim::header::Canon;

fuzz_target!(|data: &[u8]| {
    // Body canonicalization — both variants, both length-limit forms.
    let _ = canonicalize_body(data, Canon::Simple, None);
    let _ = canonicalize_body(data, Canon::Relaxed, None);
    let _ = canonicalize_body(data, Canon::Simple, Some((data.len() / 2) as u64));
    let _ = canonicalize_body(data, Canon::Relaxed, Some((data.len() / 2) as u64));

    // Header canonicalization needs (name, value) strings. Split the buffer
    // at the first null byte; both halves must be UTF-8 to feed canon_header.
    if let Some(split) = data.iter().position(|&b| b == 0) {
        if let (Ok(name), Ok(value)) = (
            std::str::from_utf8(&data[..split]),
            std::str::from_utf8(&data[split + 1..]),
        ) {
            let _ = canonicalize_header(name, value, Canon::Simple);
            let _ = canonicalize_header(name, value, Canon::Relaxed);
        }
    }
});
