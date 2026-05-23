#![no_main]
//! Fuzz the DKIM-Signature header parser. Goal: no panics on any byte
//! sequence — `parse` should return either `Ok(DkimHeader)` or
//! `Err(DkimError)`, never unwind.

use libfuzzer_sys::fuzz_target;
use mailrs_dkim::header::DkimHeader;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = DkimHeader::parse(s);
    }
});
