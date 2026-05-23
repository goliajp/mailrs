#![no_main]
//! Fuzz RFC 2047 encoded-word decoder. Charset table dispatch is bug-rich
//! historically — encoded-words come from header values which are
//! attacker-controlled.

use libfuzzer_sys::fuzz_target;
use mailrs_rfc2047::decode;

fuzz_target!(|data: &[u8]| {
    let _ = decode(data);
});
