//! Daylight saving under a recurring meeting, case by case.
//!
//! The organiser's wall-clock time is what repeats; the UTC instant moves
//! when their clocks change. A zone comes from the invitation's own
//! `VTIMEZONE` when it has one, otherwise from the `TZID` name — an IANA
//! name or a Windows one. A UTC time and an all-day date never move.

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use mailrs_ical::parse_invite;
use mailrs_ical::recur::{Moment, Series};

/// The organiser's US Pacific `VTIMEZONE`, as Exchange writes it.
const PACIFIC_2007: &str = "BEGIN:VTIMEZONE\r\nTZID:Pacific Standard Time\r\n\
BEGIN:STANDARD\r\nDTSTART:16010101T020000\r\nTZOFFSETFROM:-0700\r\nTZOFFSETTO:-0800\r\n\
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=1SU;BYMONTH=11\r\nEND:STANDARD\r\n\
BEGIN:DAYLIGHT\r\nDTSTART:16010101T020000\r\nTZOFFSETFROM:-0800\r\nTZOFFSETTO:-0700\r\n\
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=2SU;BYMONTH=3\r\nEND:DAYLIGHT\r\nEND:VTIMEZONE\r\n";

/// The same name with the pre-2007 US rules (last Sunday of October,
/// first Sunday of April) — what an old Exchange server still sends.
const PACIFIC_1987: &str = "BEGIN:VTIMEZONE\r\nTZID:Pacific Standard Time\r\n\
BEGIN:STANDARD\r\nDTSTART:16010101T020000\r\nTZOFFSETFROM:-0700\r\nTZOFFSETTO:-0800\r\n\
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=10\r\nEND:STANDARD\r\n\
BEGIN:DAYLIGHT\r\nDTSTART:16010101T020000\r\nTZOFFSETFROM:-0800\r\nTZOFFSETTO:-0700\r\n\
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=1SU;BYMONTH=4\r\nEND:DAYLIGHT\r\nEND:VTIMEZONE\r\n";

fn event(vtimezone: &str, body: &str) -> Series {
    let ics = format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\nMETHOD:REQUEST\r\n{vtimezone}\
         BEGIN:VEVENT\r\nUID:dst\r\nDTSTAMP:20261001T000000Z\r\nSUMMARY:x\r\n{body}\
         END:VEVENT\r\nEND:VCALENDAR\r\n"
    );
    let invite = parse_invite(ics.as_bytes()).expect("the fixture parses");
    Series::of(&invite).expect("a series")
}

fn utc(y: i32, m: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, h, mi, 0).unwrap()
}

fn starts(s: &Series, from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<DateTime<Utc>> {
    s.occurrences(from, to)
        .into_iter()
        .map(|o| match o.start {
            Moment::Instant(t) => t,
            Moment::Date(d) => panic!("a timed series produced a date {d}"),
        })
        .collect()
}

fn weekly_thursday_22(tzid: &str) -> String {
    format!(
        "DTSTART;TZID={tzid}:20261029T220000\r\nDTEND;TZID={tzid}:20261029T230000\r\n\
         RRULE:FREQ=WEEKLY;BYDAY=TH;COUNT=30\r\n"
    )
}

#[test]
fn the_invites_own_vtimezone_decides_the_offset() {
    // Under the pre-2007 rules Santa Clara is already on standard time on
    // 29 October, so the first meeting is 06:00 UTC, not 05:00. Only the
    // embedded block can say that; the name alone says the 2007 rules.
    let s = event(PACIFIC_1987, &weekly_thursday_22("Pacific Standard Time"));
    let got = starts(&s, utc(2026, 10, 1, 0, 0), utc(2026, 11, 7, 0, 0));
    assert_eq!(got, [utc(2026, 10, 30, 6, 0), utc(2026, 11, 6, 6, 0)]);
}

#[test]
fn a_windows_name_without_a_vtimezone_resolves_to_its_iana_zone() {
    let pacific = event("", &weekly_thursday_22("Pacific Standard Time"));
    assert_eq!(
        starts(&pacific, utc(2026, 10, 1, 0, 0), utc(2026, 11, 7, 0, 0)),
        [utc(2026, 10, 30, 5, 0), utc(2026, 11, 6, 6, 0)]
    );
    // Tokyo keeps no daylight time: the same UTC hour every week
    let tokyo = event("", &weekly_thursday_22("Tokyo Standard Time"));
    assert_eq!(
        starts(&tokyo, utc(2026, 10, 1, 0, 0), utc(2026, 11, 7, 0, 0)),
        [utc(2026, 10, 29, 13, 0), utc(2026, 11, 5, 13, 0)]
    );
    // Berlin goes back on 25 October, a week before the US
    let berlin = event(
        "",
        "DTSTART;TZID=W. Europe Standard Time:20261022T100000\r\n\
         DTEND;TZID=W. Europe Standard Time:20261022T110000\r\nRRULE:FREQ=WEEKLY;COUNT=3\r\n",
    );
    assert_eq!(
        starts(&berlin, utc(2026, 10, 1, 0, 0), utc(2026, 11, 7, 0, 0)),
        [
            utc(2026, 10, 22, 8, 0),
            utc(2026, 10, 29, 9, 0),
            utc(2026, 11, 5, 9, 0)
        ]
    );
}

#[test]
fn an_iana_name_works_as_well_as_a_windows_one() {
    let s = event("", &weekly_thursday_22("America/Los_Angeles"));
    assert_eq!(
        starts(&s, utc(2026, 10, 1, 0, 0), utc(2026, 11, 7, 0, 0)),
        [utc(2026, 10, 30, 5, 0), utc(2026, 11, 6, 6, 0)]
    );
}

#[test]
fn a_utc_series_keeps_its_utc_hour_through_the_change() {
    let s = event(
        "",
        "DTSTART:20261030T050000Z\r\nDTEND:20261030T060000Z\r\nRRULE:FREQ=WEEKLY;COUNT=3\r\n",
    );
    assert_eq!(
        starts(&s, utc(2026, 10, 1, 0, 0), utc(2026, 12, 1, 0, 0)),
        [
            utc(2026, 10, 30, 5, 0),
            utc(2026, 11, 6, 5, 0),
            utc(2026, 11, 13, 5, 0)
        ]
    );
}

/// A floating time is local to whoever reads it (RFC 5545 §3.3.5): the
/// server has no zone to place it in, so it names no instant and is not
/// expanded into any.
#[test]
fn a_floating_series_is_not_placed_in_any_zone() {
    let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\nBEGIN:VEVENT\r\nUID:f\r\n\
               DTSTAMP:20261001T000000Z\r\nDTSTART:20261029T220000\r\nRRULE:FREQ=WEEKLY\r\n\
               END:VEVENT\r\nEND:VCALENDAR\r\n";
    let invite = parse_invite(ics.as_bytes()).unwrap();
    assert!(Series::of(&invite).is_none());
    assert_eq!(mailrs_ical::instants::instants_of(&invite), (None, None));
}

/// 02:30 does not happen on 14 March 2027 in Santa Clara. RFC 5545
/// §3.3.5: it is read with the offset before the gap, which lands it at
/// 03:30 daylight time — shifted forward by the gap.
#[test]
fn a_time_the_clocks_skip_is_shifted_forward_by_the_gap() {
    let body = |tzid: &str| {
        format!(
            "DTSTART;TZID={tzid}:20270307T023000\r\nDTEND;TZID={tzid}:20270307T033000\r\n\
             RRULE:FREQ=WEEKLY;COUNT=3\r\n"
        )
    };
    let want = [
        utc(2027, 3, 7, 10, 30),
        utc(2027, 3, 14, 10, 30),
        utc(2027, 3, 21, 9, 30),
    ];
    for s in [
        event(PACIFIC_2007, &body("Pacific Standard Time")),
        event("", &body("America/Los_Angeles")),
    ] {
        assert_eq!(
            starts(&s, utc(2027, 3, 1, 0, 0), utc(2027, 3, 25, 0, 0)),
            want
        );
    }
}

/// 01:30 happens twice on 1 November 2026. RFC 5545 §3.3.5: the first
/// one, still on daylight time.
#[test]
fn a_time_that_happens_twice_is_the_first_of_the_two() {
    let body = |tzid: &str| {
        format!(
            "DTSTART;TZID={tzid}:20261025T013000\r\nDTEND;TZID={tzid}:20261025T020000\r\n\
             RRULE:FREQ=WEEKLY;COUNT=3\r\n"
        )
    };
    let want = [
        utc(2026, 10, 25, 8, 30),
        utc(2026, 11, 1, 8, 30),
        utc(2026, 11, 8, 9, 30),
    ];
    for s in [
        event(PACIFIC_2007, &body("Pacific Standard Time")),
        event("", &body("America/Los_Angeles")),
    ] {
        assert_eq!(
            starts(&s, utc(2026, 10, 20, 0, 0), utc(2026, 11, 10, 0, 0)),
            want
        );
    }
}

#[test]
fn until_as_a_local_date_ends_on_the_same_meeting_as_until_in_utc() {
    let with = |until: &str| {
        event(
            PACIFIC_2007,
            &format!(
                "DTSTART;TZID=Pacific Standard Time:20261029T220000\r\n\
                 DTEND;TZID=Pacific Standard Time:20261029T230000\r\n\
                 RRULE:FREQ=WEEKLY;UNTIL={until};BYDAY=TH\r\n"
            ),
        )
    };
    for until in ["20270430T050000Z", "20270429", "20270429T220000"] {
        let got = starts(&with(until), utc(2027, 4, 1, 0, 0), utc(2027, 6, 1, 0, 0));
        assert_eq!(got.last(), Some(&utc(2027, 4, 30, 5, 0)), "UNTIL={until}");
        assert_eq!(got.len(), 5, "UNTIL={until}");
    }
    // one minute before the last meeting's instant leaves it out
    let early = starts(
        &with("20270430T045900Z"),
        utc(2027, 4, 1, 0, 0),
        utc(2027, 6, 1, 0, 0),
    );
    assert_eq!(early.last(), Some(&utc(2027, 4, 23, 5, 0)));
}

/// An EXDATE names the occurrence it removes; it removes it whether it is
/// written in the organiser's zone or in UTC, on either side of the change.
#[test]
fn an_excluded_date_removes_its_meeting_after_the_change() {
    for exdate in [
        "EXDATE;TZID=Pacific Standard Time:20261105T220000",
        "EXDATE:20261106T060000Z",
    ] {
        let s = event(
            PACIFIC_2007,
            &format!(
                "{}{exdate}\r\n",
                weekly_thursday_22("Pacific Standard Time")
            ),
        );
        assert_eq!(
            starts(&s, utc(2026, 10, 1, 0, 0), utc(2026, 11, 14, 0, 0)),
            [utc(2026, 10, 30, 5, 0), utc(2026, 11, 13, 6, 0)],
            "{exdate}"
        );
    }
}

#[test]
fn an_all_day_series_never_moves_with_the_clocks() {
    let s = event(
        "",
        "DTSTART;VALUE=DATE:20261025\r\nDTEND;VALUE=DATE:20261026\r\nRRULE:FREQ=WEEKLY;COUNT=3\r\n",
    );
    let got: Vec<(Moment, Moment)> = s
        .occurrences(utc(2026, 10, 1, 0, 0), utc(2026, 12, 1, 0, 0))
        .into_iter()
        .map(|o| (o.start, o.end))
        .collect();
    let day = |m, d| Moment::Date(NaiveDate::from_ymd_opt(2026, m, d).unwrap());
    assert_eq!(
        got,
        [
            (day(10, 25), day(10, 26)),
            (day(11, 1), day(11, 2)),
            (day(11, 8), day(11, 9))
        ]
    );
}

/// The organiser's own clock at each occurrence: 22:00 on both sides of
/// the change, with the offset that was in force then — the line a card
/// shows as "22:00 PDT" in October and "22:00 PST" in November.
#[test]
fn the_organisers_clock_reads_22_00_with_that_dates_offset() {
    use mailrs_ical::recur::organiser_clock;
    let s = event(PACIFIC_2007, &weekly_thursday_22("Pacific Standard Time"));
    let at = |t| organiser_clock(&s.dtstart, &s.vtimezones, t).expect("a zoned start");
    let local = |m, d, h, mi| {
        NaiveDate::from_ymd_opt(2026, m, d)
            .unwrap()
            .and_hms_opt(h, mi, 0)
            .unwrap()
    };
    assert_eq!(
        at(utc(2026, 10, 30, 5, 0)),
        (local(10, 29, 22, 0), -7 * 3600)
    );
    assert_eq!(at(utc(2026, 11, 6, 6, 0)), (local(11, 5, 22, 0), -8 * 3600));
    // the repeated hour, both times round
    assert_eq!(
        at(utc(2026, 11, 1, 8, 30)),
        (local(11, 1, 1, 30), -7 * 3600)
    );
    assert_eq!(
        at(utc(2026, 11, 1, 9, 30)),
        (local(11, 1, 1, 30), -8 * 3600)
    );
    let by_name = event("", &weekly_thursday_22("Pacific Standard Time"));
    let at_named = |t| organiser_clock(&by_name.dtstart, &by_name.vtimezones, t).unwrap();
    assert_eq!(
        at_named(utc(2026, 11, 1, 9, 30)),
        (local(11, 1, 1, 30), -8 * 3600)
    );
}

#[test]
fn a_windows_name_gives_the_iana_zone_a_client_formats_with() {
    use mailrs_ical::vtimezone::iana_name;
    assert_eq!(
        iana_name("Pacific Standard Time"),
        Some("America/Los_Angeles")
    );
    assert_eq!(iana_name("Tokyo Standard Time"), Some("Asia/Tokyo"));
    assert_eq!(iana_name("W. Europe Standard Time"), Some("Europe/Berlin"));
    assert_eq!(
        iana_name("America/Los_Angeles"),
        Some("America/Los_Angeles")
    );
    assert_eq!(iana_name("(UTC-08:00) Somewhere"), None);
}
