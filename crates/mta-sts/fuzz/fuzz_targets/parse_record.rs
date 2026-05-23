#![no_main]
//! Fuzz the STS DNS TXT-record parser. Untrusted-input risk is low
//! (TXT records come from DNS, where the attacker can craft arbitrary
//! strings), but worth covering to prove the parser doesn't panic on
//! malformed UTF-8-shaped bytes.

use libfuzzer_sys::fuzz_target;
use mailrs_mta_sts::StsRecord;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = StsRecord::parse(s);
    }
});
