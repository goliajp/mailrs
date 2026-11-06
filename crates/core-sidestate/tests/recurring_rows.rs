//! A recurring event is one stored row holding its series, whoever files
//! it: a mail (and later mails about single occurrences), a feed, or a
//! CalDAV client.

use chrono::{TimeZone, Utc};
use mailrs_core_sidestate::families::calendar_events::{self as ev, series};

const VTIMEZONE: &str = "BEGIN:VTIMEZONE\r\nTZID:Pacific Standard Time\r\n\
BEGIN:STANDARD\r\nDTSTART:16010101T020000\r\nTZOFFSETFROM:-0700\r\nTZOFFSETTO:-0800\r\n\
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=1SU;BYMONTH=11\r\nEND:STANDARD\r\n\
BEGIN:DAYLIGHT\r\nDTSTART:16010101T020000\r\nTZOFFSETFROM:-0800\r\nTZOFFSETTO:-0700\r\n\
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=2SU;BYMONTH=3\r\nEND:DAYLIGHT\r\nEND:VTIMEZONE\r\n";

const MASTER: &str = "BEGIN:VEVENT\r\nUID:sync\r\nSEQUENCE:0\r\nDTSTAMP:20261028T010000Z\r\n\
SUMMARY:GOLIA sync\r\nDTSTART;TZID=Pacific Standard Time:20261029T220000\r\n\
DTEND;TZID=Pacific Standard Time:20261029T230000\r\n\
RRULE:FREQ=WEEKLY;UNTIL=20270430T050000Z;INTERVAL=1;BYDAY=TH;WKST=SU\r\nEND:VEVENT\r\n";

/// The 12 November meeting, moved to 08:00 on the Friday.
const MOVED: &str = "BEGIN:VEVENT\r\nUID:sync\r\nSEQUENCE:1\r\nDTSTAMP:20261028T010000Z\r\n\
SUMMARY:GOLIA sync\r\nRECURRENCE-ID;TZID=Pacific Standard Time:20261112T220000\r\n\
DTSTART;TZID=Pacific Standard Time:20261113T080000\r\n\
DTEND;TZID=Pacific Standard Time:20261113T090000\r\nEND:VEVENT\r\n";

fn doc(method: &str, events: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\nMETHOD:{method}\r\n{VTIMEZONE}{events}END:VCALENDAR\r\n"
    )
}

fn payload(text: &str) -> serde_json::Value {
    serde_json::to_value(mailrs_ical::parse_invite(text.as_bytes()).expect("parses")).unwrap()
}

fn utc(y: i32, m: u32, d: u32, h: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap()
}

fn starts(row: &ev::StoredEvent) -> Vec<String> {
    series::occurrences(row, utc(2026, 10, 26, 0), utc(2026, 11, 21, 0))
        .expect("a recurring row")
        .into_iter()
        .map(|o| o.start.to_column())
        .collect()
}

const FOUR_WEEKS: [&str; 4] = [
    "2026-10-30T05:00:00+00:00",
    "2026-11-06T06:00:00+00:00",
    "2026-11-13T06:00:00+00:00",
    "2026-11-20T06:00:00+00:00",
];

#[test]
fn a_feed_or_caldav_document_folds_its_moved_occurrence_into_the_series() {
    let text = doc("PUBLISH", &format!("{MASTER}{MOVED}"));
    let folded = series::fold_text(&text);
    assert_eq!(folded.len(), 1, "one event per UID");
    let row = ev::from_ics(&text, "caldav").expect("stored");
    assert_eq!(row.dtstart.as_deref(), Some("2026-10-30T05:00:00+00:00"));
    let mut want = FOUR_WEEKS.to_vec();
    want[2] = "2026-11-13T16:00:00+00:00";
    assert_eq!(starts(&row), want);
}

#[test]
fn a_mail_about_one_occurrence_changes_that_occurrence_only() {
    let mut kv = kevy_client::Connection::connect("mem://recurring-rows-mail").unwrap();
    let master = payload(&doc("REQUEST", MASTER));
    let row = ev::from_invite("sync", "REQUEST", 0, &master);
    assert!(ev::file_invitation(&mut kv, "a@x", &row, &master).unwrap());
    assert_eq!(
        starts(&ev::get(&mut kv, "a@x", "sync").unwrap().unwrap()),
        FOUR_WEEKS
    );

    let cancel = payload(&doc(
        "CANCEL",
        &MOVED.replace("SEQUENCE:1", "SEQUENCE:1\r\nSTATUS:CANCELLED"),
    ));
    let cancel_row = ev::from_invite("sync", "CANCEL", 1, &cancel);
    assert!(ev::file_invitation(&mut kv, "a@x", &cancel_row, &cancel).unwrap());
    let stored = ev::get(&mut kv, "a@x", "sync").unwrap().unwrap();
    assert_ne!(
        stored.status.as_deref(),
        Some("CANCELLED"),
        "the series is not cancelled"
    );
    assert_eq!(
        starts(&stored),
        [FOUR_WEEKS[0], FOUR_WEEKS[1], FOUR_WEEKS[3]]
    );

    // the organiser re-sends the series; the cancelled week stays cancelled
    let resent = ev::from_invite("sync", "REQUEST", 2, &master);
    assert!(ev::file_invitation(&mut kv, "a@x", &resent, &master).unwrap());
    let stored = ev::get(&mut kv, "a@x", "sync").unwrap().unwrap();
    assert_eq!(stored.sequence, 2);
    assert_eq!(
        starts(&stored),
        [FOUR_WEEKS[0], FOUR_WEEKS[1], FOUR_WEEKS[3]]
    );
}

#[test]
fn an_occurrence_with_no_series_stored_is_an_event_of_its_own() {
    let mut kv = kevy_client::Connection::connect("mem://recurring-rows-alone").unwrap();
    let alone = payload(&doc("REQUEST", MOVED));
    let mut row = ev::from_invite("sync", "REQUEST", 1, &alone);
    row.dtstart = Some("2026-11-13T16:00:00+00:00".into());
    assert!(ev::file_invitation(&mut kv, "a@x", &row, &alone).unwrap());
    let stored = ev::get(&mut kv, "a@x", "sync").unwrap().unwrap();
    assert!(stored.recurrence.is_none());
    assert!(series::occurrences(&stored, utc(2026, 1, 1, 0), utc(2028, 1, 1, 0)).is_none());
}

#[test]
fn the_card_gets_the_rule_and_the_next_occurrence_as_of_now() {
    let mut p = payload(&doc("REQUEST", MASTER));
    series::annotate(&mut p, utc(2026, 11, 6, 0));
    assert_eq!(p["organiser_zone"], "America/Los_Angeles");
    assert_eq!(p["recurrence"]["freq"], "WEEKLY");
    assert_eq!(p["recurrence"]["until"], "2027-04-29");
    assert_eq!(
        p["next_occurrence"],
        serde_json::json!({
            "start": "2026-11-06T06:00:00+00:00",
            "end": "2026-11-06T07:00:00+00:00",
            "organiser_local": "2026-11-05T22:00:00",
            "organiser_offset_secs": -8 * 3600,
        })
    );
    // the first meeting, in daylight time, still in progress
    let mut early = payload(&doc("REQUEST", MASTER));
    series::annotate(
        &mut early,
        Utc.with_ymd_and_hms(2026, 10, 30, 5, 30, 0).unwrap(),
    );
    assert_eq!(
        early["next_occurrence"]["start"],
        "2026-10-30T05:00:00+00:00"
    );
    assert_eq!(early["next_occurrence"]["organiser_offset_secs"], -7 * 3600);
    // after the last meeting there is none
    let mut ended = payload(&doc("REQUEST", MASTER));
    series::annotate(&mut ended, utc(2027, 5, 1, 0));
    assert!(ended["next_occurrence"].is_null());
    assert_eq!(ended["recurrence"]["freq"], "WEEKLY");
}

#[test]
fn a_card_for_one_occurrence_is_not_told_about_the_series() {
    let mut p = payload(&doc("REQUEST", MOVED));
    series::annotate(&mut p, utc(2026, 11, 6, 0));
    assert_eq!(p["organiser_zone"], "America/Los_Angeles");
    assert!(p.get("recurrence").is_none());
    assert!(p.get("next_occurrence").is_none());
}
