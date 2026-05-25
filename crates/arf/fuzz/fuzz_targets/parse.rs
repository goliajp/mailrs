#![no_main]
//! Fuzz mailrs-arf on arbitrary byte input — must not panic / OOM.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = mailrs_arf::parse(data);
});
