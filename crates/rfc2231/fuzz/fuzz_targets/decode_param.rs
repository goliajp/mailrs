#![no_main]
//! Fuzz mailrs-rfc2231 on arbitrary byte input — must not panic / OOM.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = mailrs_rfc2231::decode_param_value(s);
    }
});
