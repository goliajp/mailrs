#![no_main]
//! Fuzz mailrs-dav on arbitrary byte input — must not panic / OOM.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = mailrs_dav::parse::parse_depth(std::str::from_utf8(data).ok());
});
