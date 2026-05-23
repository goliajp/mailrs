#![no_main]
//! Fuzz SPF record parser. SPF records come from DNS TXT — fully attacker-
//! controlled. Goal: no panics on any input.

use libfuzzer_sys::fuzz_target;
use mailrs_spf::Record;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = Record::parse(s);
    }
});
