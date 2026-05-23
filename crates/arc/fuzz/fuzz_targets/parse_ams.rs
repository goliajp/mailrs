#![no_main]
//! Fuzz the ARC-Message-Signature header parser. Same untrusted-input
//! risk as DKIM-Signature parsing.

use libfuzzer_sys::fuzz_target;
use mailrs_arc::ArcMessageSignature;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = ArcMessageSignature::parse(s);
    }
});
