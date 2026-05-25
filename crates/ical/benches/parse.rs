//! Micro-benchmarks for the iCalendar parser + serializer.
//!
//! Run with: `cargo bench -p mailrs-ical`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use mailrs_ical::parse_invite;
use mailrs_ical::serialize::serialize;

const MINIMAL: &[u8] = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\nMETHOD:REQUEST\r\n\
BEGIN:VEVENT\r\nUID:bench-min\r\nDTSTAMP:19970714T170000Z\r\nDTSTART:19970714T170000Z\r\n\
SUMMARY:Quick sync\r\nORGANIZER:mailto:o@example.com\r\n\
ATTENDEE;CN=Alice;PARTSTAT=ACCEPTED:mailto:alice@example.com\r\n\
END:VEVENT\r\nEND:VCALENDAR\r\n";

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
SUMMARY:Quarterly Planning Workshop with Multiple Stakeholders\r\n\
LOCATION:Tokyo HQ\\, Conference Room A\r\n\
DESCRIPTION:Long description with embedded\\nnewlines and \\\\backslashes plus a comma\\, semicolon\\;\r\n\
STATUS:CONFIRMED\r\n\
ORGANIZER;CN=John Doe:mailto:jdoe@example.com\r\n\
ATTENDEE;CN=Alice;PARTSTAT=ACCEPTED;ROLE=REQ-PARTICIPANT;RSVP=TRUE:mailto:alice@example.com\r\n\
ATTENDEE;CN=Bob;PARTSTAT=DECLINED;ROLE=OPT-PARTICIPANT;RSVP=FALSE:mailto:bob@example.com\r\n\
ATTENDEE;CN=Carol;PARTSTAT=TENTATIVE;ROLE=REQ-PARTICIPANT;RSVP=TRUE:mailto:carol@example.com\r\n\
ATTENDEE;CN=Dave;PARTSTAT=NEEDS-ACTION;ROLE=NON-PARTICIPANT:mailto:dave@example.com\r\n\
END:VEVENT\r\nEND:VCALENDAR\r\n";

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_invite");
    group.bench_function("minimal", |b| b.iter(|| parse_invite(black_box(MINIMAL))));
    group.bench_function("complex", |b| b.iter(|| parse_invite(black_box(COMPLEX))));
    group.finish();
}

fn bench_serialize(c: &mut Criterion) {
    let minimal_invite = parse_invite(MINIMAL).unwrap();
    let complex_invite = parse_invite(COMPLEX).unwrap();

    let mut group = c.benchmark_group("serialize");
    group.bench_function("minimal", |b| {
        b.iter(|| serialize(black_box(&minimal_invite)))
    });
    group.bench_function("complex", |b| {
        b.iter(|| serialize(black_box(&complex_invite)))
    });
    group.finish();
}

fn bench_round_trip(c: &mut Criterion) {
    let mut group = c.benchmark_group("round_trip");
    group.bench_function("complex", |b| {
        b.iter(|| {
            let inv = parse_invite(black_box(COMPLEX)).unwrap();
            serialize(&inv).unwrap()
        })
    });
    group.finish();
}

criterion_group!(benches, bench_parse, bench_serialize, bench_round_trip);
criterion_main!(benches);
