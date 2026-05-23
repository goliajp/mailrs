//! Head-to-head: `mailrs-ical` vs `icalendar` 0.17.
//!
//! Both parse RFC 5545 iCalendar text into a structured tree.
//! `icalendar` is the most popular crate in this space; we benchmark the
//! parse step (zero-I/O, identical input) — *not* downstream method calls.
//!
//! Workloads:
//!
//! * `simple_vevent` — single VEVENT with the typical 8 properties an
//!   invitation has.
//! * `recurring_vevent` — same plus RRULE; pressures recurrence parsing.
//! * `vtimezone_heavy` — full VTIMEZONE block + VEVENT inside.

use criterion::{Criterion, criterion_group, criterion_main};
use icalendar::Calendar;
use mailrs_ical::parse::parse_calendar;
use std::hint::black_box;
use std::str::FromStr;

const SIMPLE: &str = "BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
PRODID:-//Mailrs//EN\r\n\
BEGIN:VEVENT\r\n\
UID:abc123@example.com\r\n\
DTSTAMP:20260101T120000Z\r\n\
DTSTART:20260201T140000Z\r\n\
DTEND:20260201T150000Z\r\n\
SUMMARY:Quarterly review\r\n\
DESCRIPTION:Discuss Q1 results\r\n\
ORGANIZER;CN=\"Alice\":mailto:alice@example.com\r\n\
ATTENDEE;CN=\"Bob\";RSVP=TRUE:mailto:bob@example.com\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

const RECURRING: &str = "BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
PRODID:-//Mailrs//EN\r\n\
BEGIN:VEVENT\r\n\
UID:weekly-standup@example.com\r\n\
DTSTAMP:20260101T120000Z\r\n\
DTSTART:20260105T100000Z\r\n\
DTEND:20260105T103000Z\r\n\
SUMMARY:Weekly standup\r\n\
RRULE:FREQ=WEEKLY;BYDAY=MO;COUNT=52\r\n\
ATTENDEE;CN=\"Alice\":mailto:alice@example.com\r\n\
ATTENDEE;CN=\"Bob\":mailto:bob@example.com\r\n\
ATTENDEE;CN=\"Carol\":mailto:carol@example.com\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

const VTIMEZONE_HEAVY: &str = "BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
PRODID:-//Mailrs//EN\r\n\
BEGIN:VTIMEZONE\r\n\
TZID:America/Los_Angeles\r\n\
BEGIN:STANDARD\r\n\
DTSTART:20071104T020000\r\n\
TZOFFSETFROM:-0700\r\n\
TZOFFSETTO:-0800\r\n\
TZNAME:PST\r\n\
RRULE:FREQ=YEARLY;BYDAY=1SU;BYMONTH=11\r\n\
END:STANDARD\r\n\
BEGIN:DAYLIGHT\r\n\
DTSTART:20070311T020000\r\n\
TZOFFSETFROM:-0800\r\n\
TZOFFSETTO:-0700\r\n\
TZNAME:PDT\r\n\
RRULE:FREQ=YEARLY;BYDAY=2SU;BYMONTH=3\r\n\
END:DAYLIGHT\r\n\
END:VTIMEZONE\r\n\
BEGIN:VEVENT\r\n\
UID:tz@example.com\r\n\
DTSTAMP:20260101T120000Z\r\n\
DTSTART;TZID=America/Los_Angeles:20260301T090000\r\n\
DTEND;TZID=America/Los_Angeles:20260301T100000\r\n\
SUMMARY:Coffee chat\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

fn bench_simple(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse/simple_vevent");
    group.bench_function("mailrs_ical", |b| {
        b.iter(|| {
            let r = parse_calendar(black_box(SIMPLE));
            black_box(r.unwrap())
        });
    });
    group.bench_function("icalendar", |b| {
        b.iter(|| {
            let r = Calendar::from_str(black_box(SIMPLE));
            black_box(r.unwrap())
        });
    });
    group.finish();
}

fn bench_recurring(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse/recurring_vevent");
    group.bench_function("mailrs_ical", |b| {
        b.iter(|| {
            let r = parse_calendar(black_box(RECURRING));
            black_box(r.unwrap())
        });
    });
    group.bench_function("icalendar", |b| {
        b.iter(|| {
            let r = Calendar::from_str(black_box(RECURRING));
            black_box(r.unwrap())
        });
    });
    group.finish();
}

fn bench_vtimezone(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse/vtimezone_heavy");
    group.bench_function("mailrs_ical", |b| {
        b.iter(|| {
            let r = parse_calendar(black_box(VTIMEZONE_HEAVY));
            black_box(r.unwrap())
        });
    });
    group.bench_function("icalendar", |b| {
        b.iter(|| {
            let r = Calendar::from_str(black_box(VTIMEZONE_HEAVY));
            black_box(r.unwrap())
        });
    });
    group.finish();
}

criterion_group!(benches, bench_simple, bench_recurring, bench_vtimezone);
criterion_main!(benches);
