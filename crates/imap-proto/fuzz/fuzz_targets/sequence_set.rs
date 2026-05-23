#![no_main]
//! Fuzz IMAP sequence-set parser. Range syntax with `*` wildcards is a
//! classic source of off-by-one bugs.

use libfuzzer_sys::fuzz_target;
use mailrs_imap_proto::parse_sequence_set;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = parse_sequence_set(s);
    }
});
