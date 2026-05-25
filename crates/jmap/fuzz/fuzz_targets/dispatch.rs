#![no_main]
//! Fuzz mailrs-jmap on arbitrary byte input — must not panic / OOM.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _: Result<mailrs_jmap::dispatch::JmapRequest, _> = serde_json::from_str(s);
    }
});
