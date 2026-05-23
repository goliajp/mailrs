#![no_main]
//! Fuzz iCalendar (RFC 5545) parser. Recursive nested components +
//! RRULE expansion are common bug sources.

use libfuzzer_sys::fuzz_target;
use mailrs_ical::parse::parse_calendar;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = parse_calendar(s);
    }
});
