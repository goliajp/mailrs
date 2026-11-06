//! A series written out for a calendar client (CalDAV) and read back gives
//! the same occurrences at the same instants, across both clock changes.
//!
//! The series is the Teams meeting of `a_weekly_meeting_moves_with_the_clocks`:
//! Thursdays at 22:00 `Pacific Standard Time`, 2026-10-29 to 2027-04-29.

use chrono::{DateTime, TimeZone, Utc};
use mailrs_ical::parse::split_vevents;
use mailrs_ical::parse_invite;
use mailrs_ical::recur::{Head, Moment, Occurrence, Override, Series};

const VTIMEZONE: &str = "BEGIN:VTIMEZONE\r\n\
TZID:Pacific Standard Time\r\n\
BEGIN:STANDARD\r\n\
DTSTART:16010101T020000\r\n\
TZOFFSETFROM:-0700\r\n\
TZOFFSETTO:-0800\r\n\
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=1SU;BYMONTH=11\r\n\
END:STANDARD\r\n\
BEGIN:DAYLIGHT\r\n\
DTSTART:16010101T020000\r\n\
TZOFFSETFROM:-0800\r\n\
TZOFFSETTO:-0700\r\n\
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=2SU;BYMONTH=3\r\n\
END:DAYLIGHT\r\n\
END:VTIMEZONE\r\n";

const WEEKLY: &str = "RRULE:FREQ=WEEKLY;UNTIL=20270430T050000Z;INTERVAL=1;BYDAY=TH;WKST=SU\r\n";

fn invite(zone_block: &str, tzid: &str, extra: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nVERSION:2.0\r\n{zone_block}\
BEGIN:VEVENT\r\nORGANIZER;CN=Chair:mailto:chair@example.com\r\nUID:sync\r\n\
SUMMARY:GOLIA sync\r\nDTSTART;TZID={tzid}:20261029T220000\r\n\
DTEND;TZID={tzid}:20261029T230000\r\n{extra}DTSTAMP:20261028T010000Z\r\n\
SEQUENCE:0\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}

/// The series a document describes, with its moved and cancelled
/// occurrences attached, as a client storing it would read it.
fn read(text: &str) -> Series {
    let events: Vec<_> = split_vevents(text)
        .iter()
        .map(|d| parse_invite(d.as_bytes()).expect("each VEVENT parses"))
        .collect();
    let master = events
        .iter()
        .find(|e| e.recurrence_id.is_none())
        .expect("a master");
    let mut s = Series::of(master).expect("a series");
    for inst in events.iter().filter(|e| e.recurrence_id.is_some()) {
        s.attach(Override::of(inst).expect("an override"));
    }
    s
}

fn head() -> Head<'static> {
    Head {
        uid: "sync",
        summary: "GOLIA sync",
        organizer: Some("chair@example.com"),
        status: None,
        sequence: 0,
        dtstamp: "20261030T050000Z",
    }
}

fn utc(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap()
}

fn all(s: &Series) -> Vec<Occurrence> {
    s.occurrences(utc(2026, 1, 1, 0), utc(2028, 1, 1, 0))
}

fn starts(s: &Series) -> Vec<Moment> {
    all(s).into_iter().map(|o| o.start).collect()
}

/// Write `s` out, read it back, and check every occurrence (start, end,
/// which occurrence of the rule it is) is unchanged. Returns the text.
fn round_trip(s: &Series) -> String {
    let text = s.to_calendar(&head()).expect("written");
    let back = read(&text);
    assert_eq!(all(&back), all(s), "{text}");
    text
}

fn the_four_instants_are_there(s: &Series) {
    let got = starts(s);
    for want in [
        utc(2026, 10, 30, 5),
        utc(2026, 11, 6, 6),
        utc(2027, 3, 12, 6),
        utc(2027, 3, 19, 5),
    ] {
        assert!(got.contains(&Moment::Instant(want)), "{want} in {got:?}");
    }
}

#[test]
fn the_organisers_own_zone_is_written_back_as_they_sent_it() {
    let s = read(&invite(VTIMEZONE, "Pacific Standard Time", WEEKLY));
    let text = round_trip(&s);
    the_four_instants_are_there(&read(&text));
    assert!(text.contains(VTIMEZONE), "{text}");
    assert!(
        text.contains("DTSTART;TZID=Pacific Standard Time:20261029T220000\r\n"),
        "{text}"
    );
    assert!(
        text.contains("RRULE:FREQ=WEEKLY;INTERVAL=1;BYDAY=TH;WKST=SU;UNTIL=20270430T050000Z\r\n"),
        "{text}"
    );
    assert_eq!(all(&read(&text)).len(), 27);
}

/// No `VTIMEZONE` in the invitation: the Windows name is written as the
/// IANA zone it means, with a definition generated for it.
#[test]
fn a_zone_known_only_by_its_windows_name_is_written_as_its_iana_zone() {
    let s = read(&invite("", "Pacific Standard Time", WEEKLY));
    let text = round_trip(&s);
    the_four_instants_are_there(&read(&text));
    assert!(text.contains("TZID:America/Los_Angeles\r\n"), "{text}");
    assert!(
        text.contains("DTSTART;TZID=America/Los_Angeles:20261029T220000\r\n"),
        "{text}"
    );
    assert!(
        text.contains("DTEND;TZID=America/Los_Angeles:20261029T230000\r\n"),
        "{text}"
    );
    assert!(!text.contains("Pacific Standard Time"), "{text}");
    assert!(text.contains("RRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=2SU\r\n"));
    assert!(text.contains("RRULE:FREQ=YEARLY;BYMONTH=11;BYDAY=1SU\r\n"));
}

#[test]
fn an_iana_zone_with_no_definition_gets_one() {
    let s = read(&invite("", "America/Los_Angeles", WEEKLY));
    let text = round_trip(&s);
    the_four_instants_are_there(&read(&text));
    assert_eq!(text.matches("BEGIN:VTIMEZONE").count(), 1);
}

/// `UNTIL` must be UTC when `DTSTART` has a zone; a producer that wrote
/// it as local time or as a date still ends on the same meeting.
#[test]
fn until_written_as_local_time_or_a_date_is_written_back_in_utc() {
    for until in ["20270429T220000", "20270429"] {
        let rule = format!("RRULE:FREQ=WEEKLY;UNTIL={until};BYDAY=TH\r\n");
        let s = read(&invite(VTIMEZONE, "Pacific Standard Time", &rule));
        let text = round_trip(&s);
        let expected = match until.len() {
            8 => "UNTIL=20270430T065959Z",
            _ => "UNTIL=20270430T050000Z",
        };
        assert!(text.contains(expected), "{until}: {text}");
        let back = read(&text);
        assert_eq!(
            starts(&back).last(),
            Some(&Moment::Instant(utc(2027, 4, 30, 5)))
        );
    }
}

#[test]
fn excluded_moved_and_cancelled_occurrences_survive_the_round_trip() {
    let extra = format!(
        "{WEEKLY}EXDATE:20261106T060000Z\r\nRDATE;TZID=Pacific Standard Time:20261102T090000\r\n"
    );
    let mut s = read(&invite(VTIMEZONE, "Pacific Standard Time", &extra));
    let tz = |local: &str| mailrs_ical::CalDateTime::Zoned {
        tz_name: "Pacific Standard Time".into(),
        local: chrono::NaiveDateTime::parse_from_str(local, "%Y%m%dT%H%M%S").unwrap(),
    };
    s.attach(Override {
        recurrence_id: tz("20261112T220000"),
        dtstart: tz("20261113T080000"),
        length_secs: 3600,
        cancelled: false,
    });
    s.attach(Override {
        recurrence_id: tz("20261119T220000"),
        dtstart: tz("20261119T220000"),
        length_secs: 3600,
        cancelled: true,
    });
    let text = round_trip(&s);
    // the exclusion was in UTC; it is written in the series' zone
    assert!(
        text.contains("EXDATE;TZID=Pacific Standard Time:20261105T220000\r\n"),
        "{text}"
    );
    assert!(
        text.contains("EXDATE;TZID=Pacific Standard Time:20261119T220000\r\n"),
        "{text}"
    );
    assert!(
        text.contains("RDATE;TZID=Pacific Standard Time:20261102T090000\r\n"),
        "{text}"
    );
    assert!(
        text.contains("RECURRENCE-ID;TZID=Pacific Standard Time:20261112T220000\r\n"),
        "{text}"
    );
    let got: Vec<Moment> = read(&text)
        .occurrences(utc(2026, 10, 26, 0), utc(2026, 11, 28, 0))
        .into_iter()
        .map(|o| o.start)
        .collect();
    assert_eq!(
        got,
        [
            utc(2026, 10, 30, 5),
            utc(2026, 11, 2, 17),
            utc(2026, 11, 13, 16),
            utc(2026, 11, 27, 6)
        ]
        .map(Moment::Instant)
    );
}

#[test]
fn an_all_day_series_is_written_as_dates() {
    let text = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:bins\r\n\
DTSTAMP:20261028T010000Z\r\nSUMMARY:Bins\r\nDTSTART;VALUE=DATE:20261029\r\n\
DTEND;VALUE=DATE:20261030\r\nRRULE:FREQ=WEEKLY;UNTIL=20270429T070000Z;BYDAY=TH\r\n\
EXDATE;VALUE=DATE:20261105\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let s = read(text);
    let out = round_trip(&s);
    for line in [
        "DTSTART;VALUE=DATE:20261029\r\n",
        "DTEND;VALUE=DATE:20261030\r\n",
        "UNTIL=20270429\r\n",
        "EXDATE;VALUE=DATE:20261105\r\n",
    ] {
        assert!(out.contains(line), "{line} in {out}");
    }
    assert!(!out.contains("VTIMEZONE"), "{out}");
    assert!(!out.contains("T000000"), "no date became a time: {out}");
    assert_eq!(all(&read(&out)).len(), 26);
}

#[test]
fn a_utc_series_stays_in_utc() {
    let text = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:u\r\n\
DTSTAMP:20261028T010000Z\r\nSUMMARY:U\r\nDTSTART:20261029T120000Z\r\n\
DURATION:PT30M\r\nRRULE:FREQ=DAILY;COUNT=5\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let out = round_trip(&read(text));
    assert!(out.contains("DTSTART:20261029T120000Z\r\n"), "{out}");
    assert!(out.contains("DTEND:20261029T123000Z\r\n"), "{out}");
    assert!(out.contains("RRULE:FREQ=DAILY;COUNT=5\r\n"), "{out}");
}
