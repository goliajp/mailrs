#![no_main]
//! Fuzz SMTP command parser. Untrusted client input on the wire.

use libfuzzer_sys::fuzz_target;
use mailrs_smtp_proto::parse_command;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = parse_command(s);
    }
});
