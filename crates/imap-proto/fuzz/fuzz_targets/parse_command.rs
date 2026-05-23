#![no_main]
//! Fuzz IMAP command parser — `<tag> <command>` lines from untrusted clients.

use libfuzzer_sys::fuzz_target;
use mailrs_imap_proto::parse_command;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = parse_command(s);
    }
});
