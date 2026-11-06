//! A recurring meeting filed from mail reaches a CalDAV client as its
//! whole series: the client sees every week, at the right hour on both
//! sides of each clock change, with the weeks the organiser moved or
//! cancelled.

use std::sync::Mutex;

use chrono::{DateTime, TimeZone, Utc};
use kevy_client::Connection;
use mailrs_core_sidestate::families::calendar_events::{self as ev, series};
use mailrs_core_sidestate::families::dav_store::{DavStore, SingleConn};
use mailrs_dav::dispatch::{DavRequest, handle};
use mailrs_ical::recur::Moment;

const USER: &str = "a@x.test";

/// The organiser's own zone definition, as Exchange sends it.
const VTIMEZONE: &str = "BEGIN:VTIMEZONE\r\nTZID:Pacific Standard Time\r\n\
BEGIN:STANDARD\r\nDTSTART:16010101T020000\r\nTZOFFSETFROM:-0700\r\nTZOFFSETTO:-0800\r\n\
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=1SU;BYMONTH=11\r\nEND:STANDARD\r\n\
BEGIN:DAYLIGHT\r\nDTSTART:16010101T020000\r\nTZOFFSETFROM:-0800\r\nTZOFFSETTO:-0700\r\n\
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=2SU;BYMONTH=3\r\nEND:DAYLIGHT\r\nEND:VTIMEZONE\r\n";

const MASTER: &str = "BEGIN:VEVENT\r\nUID:sync\r\nSEQUENCE:0\r\nDTSTAMP:20261028T010000Z\r\n\
ORGANIZER:mailto:chair@x.test\r\nSUMMARY:GOLIA sync\r\n\
DTSTART;TZID=Pacific Standard Time:20261029T220000\r\n\
DTEND;TZID=Pacific Standard Time:20261029T230000\r\n\
RRULE:FREQ=WEEKLY;UNTIL=20270430T050000Z;INTERVAL=1;BYDAY=TH;WKST=SU\r\nEND:VEVENT\r\n";

/// The 12 November meeting, moved to 08:00 on the Friday.
const MOVED: &str = "BEGIN:VEVENT\r\nUID:sync\r\nSEQUENCE:1\r\nDTSTAMP:20261028T010000Z\r\n\
SUMMARY:GOLIA sync\r\nRECURRENCE-ID;TZID=Pacific Standard Time:20261112T220000\r\n\
DTSTART;TZID=Pacific Standard Time:20261113T080000\r\n\
DTEND;TZID=Pacific Standard Time:20261113T090000\r\nEND:VEVENT\r\n";

fn doc(method: &str, zone: &str, events: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\nMETHOD:{method}\r\n{zone}{events}END:VCALENDAR\r\n"
    )
}

fn payload(text: &str) -> serde_json::Value {
    serde_json::to_value(mailrs_ical::parse_invite(text.as_bytes()).expect("parses")).unwrap()
}

fn utc(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap()
}

fn file(kv: &mut Connection, method: &str, seq: i32, text: &str) {
    let p = payload(text);
    let mut row = ev::from_invite("sync", method, seq, &p);
    row.dtstart = Some("2026-10-30T05:00:00+00:00".into());
    row.dtend = Some("2026-10-30T06:00:00+00:00".into());
    assert!(ev::file_invitation(kv, USER, &row, &p).expect("filed"));
}

fn starts(row: &ev::StoredEvent) -> Vec<Moment> {
    series::occurrences(row, utc(2026, 1, 1, 0), utc(2028, 1, 1, 0))
        .expect("a recurring row")
        .into_iter()
        .map(|o| o.start)
        .collect()
}

/// What a client gets from GET reads back as the same series.
fn round_trips(row: &ev::StoredEvent) -> String {
    let text = ev::to_ics(row);
    let back = ev::from_ics(&text, "caldav").expect("the served text parses");
    assert_eq!(starts(&back), starts(row), "{text}");
    for want in [
        utc(2026, 10, 30, 5),
        utc(2026, 11, 6, 6),
        utc(2027, 3, 12, 6),
        utc(2027, 3, 19, 5),
    ] {
        assert!(starts(&back).contains(&Moment::Instant(want)), "{want}");
    }
    text
}

#[test]
fn a_mail_filed_series_is_served_with_its_rule_and_zone() {
    for (name, zone) in [("own", VTIMEZONE), ("by-name", "")] {
        let mut kv = Connection::connect(&format!("mem://caldav-recurring-{name}")).unwrap();
        file(&mut kv, "REQUEST", 0, &doc("REQUEST", zone, MASTER));
        let row = ev::get(&mut kv, USER, "sync").unwrap().unwrap();
        let text = round_trips(&row);
        assert!(text.contains("RRULE:FREQ=WEEKLY"), "{text}");
        assert!(text.contains("UNTIL=20270430T050000Z"), "{text}");
        assert_eq!(text.matches("BEGIN:VTIMEZONE").count(), 1, "{text}");
        assert_eq!(text, ev::to_ics(&row), "the same row, the same bytes");
    }
}

#[test]
fn moved_and_cancelled_weeks_from_later_mails_are_served() {
    let mut kv = Connection::connect("mem://caldav-recurring-overrides").unwrap();
    file(&mut kv, "REQUEST", 0, &doc("REQUEST", VTIMEZONE, MASTER));
    file(&mut kv, "REQUEST", 1, &doc("REQUEST", VTIMEZONE, MOVED));
    let cancelled = MOVED
        .replace("20261112T220000", "20261119T220000")
        .replace("20261113T080000", "20261119T220000")
        .replace("20261113T090000", "20261119T230000");
    file(&mut kv, "CANCEL", 1, &doc("CANCEL", VTIMEZONE, &cancelled));
    let row = ev::get(&mut kv, USER, "sync").unwrap().unwrap();
    let text = round_trips(&row);
    assert!(
        text.contains("RECURRENCE-ID;TZID=Pacific Standard Time:20261112T220000\r\n"),
        "{text}"
    );
    assert!(
        text.contains("EXDATE;TZID=Pacific Standard Time:20261119T220000\r\n"),
        "{text}"
    );
    let back = ev::from_ics(&text, "caldav").unwrap();
    let november: Vec<Moment> =
        series::occurrences(&back, utc(2026, 11, 10, 0), utc(2026, 11, 28, 0))
            .unwrap()
            .into_iter()
            .map(|o| o.start)
            .collect();
    assert_eq!(
        november,
        [utc(2026, 11, 13, 16), utc(2026, 11, 27, 6)].map(Moment::Instant)
    );
}

fn store(url: &str) -> DavStore<SingleConn> {
    DavStore::new(
        USER,
        SingleConn(Mutex::new(Connection::connect(url).expect("kevy"))),
    )
}

async fn send(
    store: &DavStore<SingleConn>,
    method: &str,
    path: &str,
    body: &str,
) -> (String, Option<String>) {
    let req = DavRequest {
        method,
        path,
        depth: Some("1"),
        if_match: None,
        if_none_match: None,
        body,
    };
    let r = handle(&req, USER, store, store).await.expect(method);
    let etag = r
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("etag"))
        .map(|(_, v)| v.to_string());
    (String::from_utf8(r.body).expect("utf8"), etag)
}

/// A time-range query for the week of 5 November — weeks after the first
/// meeting — returns the series.
#[tokio::test]
async fn a_time_range_report_for_a_later_week_returns_the_series() {
    let url = "mem://caldav-recurring-report";
    let mut kv = Connection::connect(url).unwrap();
    file(&mut kv, "REQUEST", 0, &doc("REQUEST", "", MASTER));
    let s = store(url);
    let query = "<C:calendar-query xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:caldav\">\
<D:prop><D:getetag/><C:calendar-data/></D:prop>\
<C:filter><C:comp-filter name=\"VCALENDAR\"><C:comp-filter name=\"VEVENT\">\
<C:time-range start=\"20261105T000000Z\" end=\"20261112T000000Z\"/>\
</C:comp-filter></C:comp-filter></C:filter></C:calendar-query>";
    let (body, _) = send(
        &s,
        "REPORT",
        &format!("/dav/calendars/{USER}/default/"),
        query,
    )
    .await;
    assert!(body.contains("default/sync.ics"), "{body}");
    assert!(body.contains("RRULE:FREQ=WEEKLY"), "{body}");
}

/// A client re-fetches an event only when its ETag changes: a moved week
/// and a change to the series must both change it.
#[tokio::test]
async fn the_etag_changes_with_the_series_and_with_one_week() {
    let url = "mem://caldav-recurring-etag";
    let mut kv = Connection::connect(url).unwrap();
    let s = store(url);
    let path = format!("/dav/calendars/{USER}/default/sync.ics");
    file(&mut kv, "REQUEST", 0, &doc("REQUEST", VTIMEZONE, MASTER));
    let (_, first) = send(&s, "GET", &path, "").await;
    file(&mut kv, "REQUEST", 1, &doc("REQUEST", VTIMEZONE, MOVED));
    let (body, moved) = send(&s, "GET", &path, "").await;
    assert!(body.contains("RECURRENCE-ID"), "{body}");
    let resent = MASTER.replace("SEQUENCE:0", "SEQUENCE:2").replace(
        "WKST=SU\r\n",
        "WKST=SU\r\nEXDATE;TZID=Pacific Standard Time:20261203T220000\r\n",
    );
    file(&mut kv, "REQUEST", 2, &doc("REQUEST", VTIMEZONE, &resent));
    let (body, excluded) = send(&s, "GET", &path, "").await;
    assert!(
        body.contains("EXDATE;TZID=Pacific Standard Time:20261203T220000"),
        "{body}"
    );
    assert!(
        body.contains("RECURRENCE-ID"),
        "the moved week is kept: {body}"
    );
    assert!(first.is_some());
    assert_ne!(first, moved);
    assert_ne!(moved, excluded);
}
