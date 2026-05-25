//! Performance regression gates. See [BUDGETS.md](../BUDGETS.md).

use std::time::{Duration, Instant};

use mailrs_ical::parse_invite;
use mailrs_ical::serialize::serialize;

const ITERS: usize = 100;

fn time_median<F: FnMut()>(mut op: F) -> Duration {
    let mut samples = Vec::with_capacity(ITERS);
    for _ in 0..ITERS {
        let start = Instant::now();
        op();
        samples.push(start.elapsed());
    }
    samples.sort();
    samples[ITERS / 2]
}

const COMPLEX: &[u8] = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Outlook//EN\r\nMETHOD:REQUEST\r\n\
BEGIN:VTIMEZONE\r\nTZID:America/New_York\r\n\
BEGIN:STANDARD\r\nDTSTART:19701101T020000\r\nTZOFFSETFROM:-0400\r\nTZOFFSETTO:-0500\r\nRRULE:FREQ=YEARLY;BYMONTH=11;BYDAY=1SU\r\nEND:STANDARD\r\n\
BEGIN:DAYLIGHT\r\nDTSTART:19700308T020000\r\nTZOFFSETFROM:-0500\r\nTZOFFSETTO:-0400\r\nRRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=2SU\r\nEND:DAYLIGHT\r\n\
END:VTIMEZONE\r\n\
BEGIN:VEVENT\r\nUID:bench-complex\r\nDTSTAMP:19970714T170000Z\r\nSEQUENCE:4\r\n\
DTSTART;TZID=America/New_York:19980119T020000\r\n\
DTEND;TZID=America/New_York:19980119T030000\r\n\
RRULE:FREQ=WEEKLY;BYDAY=MO,WE,FR;COUNT=20\r\n\
EXDATE;TZID=America/New_York:19980126T020000\r\n\
SUMMARY:Quarterly Planning Workshop\r\n\
ORGANIZER:mailto:o@example.com\r\n\
ATTENDEE;CN=Alice;PARTSTAT=ACCEPTED:mailto:alice@example.com\r\n\
ATTENDEE;CN=Bob;PARTSTAT=DECLINED:mailto:bob@example.com\r\n\
ATTENDEE;CN=Carol;PARTSTAT=TENTATIVE:mailto:carol@example.com\r\n\
END:VEVENT\r\nEND:VCALENDAR\r\n";

#[test]
fn parse_invite_complex_under_budget() {
    let median = time_median(|| {
        let _ = parse_invite(COMPLEX);
    });
    // Budget: 1 ms. Observed P95: ~50 µs.
    let budget = Duration::from_millis(1);
    assert!(
        median < budget,
        "parse_invite(complex) median {median:?} exceeded {budget:?}"
    );
}

#[test]
fn round_trip_complex_under_budget() {
    let median = time_median(|| {
        let inv = parse_invite(COMPLEX).unwrap();
        let _ = serialize(&inv).unwrap();
    });
    // Budget: 2 ms. Observed P95: ~100 µs.
    let budget = Duration::from_millis(2);
    assert!(
        median < budget,
        "round_trip(complex) median {median:?} exceeded {budget:?}"
    );
}
