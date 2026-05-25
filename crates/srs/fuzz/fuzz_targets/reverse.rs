#![no_main]
//! Fuzz mailrs-srs on arbitrary byte input — must not panic / OOM.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) { let _ = mailrs_srs::reverse(s, "secret", 14); }
});
