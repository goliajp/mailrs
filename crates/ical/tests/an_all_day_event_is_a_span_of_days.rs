//! An all-day event is a run of dates, not an instant: it has no offset to
//! resolve, and the reader's calendar puts it on those days wherever the
//! reader is.

use chrono::NaiveDate;
use mailrs_ical::instants::all_day_of;
use mailrs_ical::parse_invite;

fn event(lines: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:u1\r\nSUMMARY:Holiday\r\nDTSTAMP:20261101T000000Z\r\n{lines}END:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}

fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).expect("date")
}

#[test]
fn an_all_day_event_covers_its_dates_with_an_exclusive_end() {
    let one = parse_invite(event("DTSTART;VALUE=DATE:20261103\r\n").as_bytes()).expect("parse");
    assert_eq!(all_day_of(&one), Some((day(2026, 11, 3), day(2026, 11, 4))));

    let three = parse_invite(
        event("DTSTART;VALUE=DATE:20261103\r\nDTEND;VALUE=DATE:20261106\r\n").as_bytes(),
    )
    .expect("parse");
    assert_eq!(all_day_of(&three), Some((day(2026, 11, 3), day(2026, 11, 6))));

    let by_duration =
        parse_invite(event("DTSTART;VALUE=DATE:20261103\r\nDURATION:P2D\r\n").as_bytes())
            .expect("parse");
    assert_eq!(all_day_of(&by_duration), Some((day(2026, 11, 3), day(2026, 11, 5))));
}

#[test]
fn an_event_at_a_time_of_day_is_not_all_day() {
    let timed = parse_invite(
        event("DTSTART:20261103T010000Z\r\nDTEND:20261103T020000Z\r\n").as_bytes(),
    )
    .expect("parse");
    assert_eq!(all_day_of(&timed), None);
}
