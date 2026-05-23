#![no_main]
//! Fuzz `parse_invite` — the high-level iTIP invitation parser.

use libfuzzer_sys::fuzz_target;
use mailrs_ical::parse_invite;

fuzz_target!(|data: &[u8]| {
    let _ = parse_invite(data);
});
