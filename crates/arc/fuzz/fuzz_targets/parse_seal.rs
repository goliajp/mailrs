#![no_main]
//! Fuzz the ARC-Seal header parser.

use libfuzzer_sys::fuzz_target;
use mailrs_arc::ArcSeal;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = ArcSeal::parse(s);
    }
});
