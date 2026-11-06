//! Every occurrence of a recurring meeting, at the instant it actually is.
//!
//! The fixture is the shape of a real Teams invitation: a weekly meeting
//! on Thursdays at 22:00 in `Pacific Standard Time` (a Windows zone name,
//! with the organiser's own `VTIMEZONE`), effective 2026-10-29 until
//! 2027-04-29. The first occurrence is in daylight time, so it is 05:00
//! UTC on the Friday; the US clocks go back on 2026-11-01, so from the
//! second occurrence on the same 22:00 is 06:00 UTC. Repeating the first
//! occurrence's instant every seven days puts every remaining meeting an
//! hour early for a reader in Tokyo.

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use mailrs_ical::parse_invite;
use mailrs_ical::recur::{Moment, Override, Series};

fn teams(extra: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\n\
METHOD:REQUEST\r\n\
PRODID:Microsoft Exchange Server 2010\r\n\
VERSION:2.0\r\n\
BEGIN:VTIMEZONE\r\n\
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
END:VTIMEZONE\r\n\
BEGIN:VEVENT\r\n\
ORGANIZER;CN=Chair:mailto:chair@example.com\r\n\
UID:040000008200E00074C5B7101A82E00800000000SYNC\r\n\
SUMMARY;LANGUAGE=en-US:GOLIA sync\r\n\
DTSTART;TZID=Pacific Standard Time:20261029T220000\r\n\
DTEND;TZID=Pacific Standard Time:20261029T230000\r\n\
{extra}\
DTSTAMP:20261028T010000Z\r\n\
SEQUENCE:0\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n"
    )
}

const WEEKLY: &str = "RRULE:FREQ=WEEKLY;UNTIL=20270430T050000Z;INTERVAL=1;BYDAY=TH;WKST=SU\r\n";

fn series(extra: &str) -> Series {
    let invite = parse_invite(teams(extra).as_bytes()).expect("the fixture parses");
    Series::of(&invite).expect("a recurring invite is a series")
}

fn utc(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap()
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

#[test]
fn the_second_occurrence_is_an_hour_later_in_utc_after_the_clocks_go_back() {
    let s = series(WEEKLY);
    let got = starts(&s, utc(2026, 10, 1, 0), utc(2026, 11, 14, 0));
    assert_eq!(
        got,
        [
            utc(2026, 10, 30, 5),
            utc(2026, 11, 6, 6),
            utc(2026, 11, 13, 6)
        ]
    );
    let first = &s.occurrences(utc(2026, 10, 1, 0), utc(2026, 11, 1, 0))[0];
    assert_eq!(first.end, Moment::Instant(utc(2026, 10, 30, 6)));
}

#[test]
fn the_clocks_going_forward_in_march_bring_it_back_to_five() {
    let s = series(WEEKLY);
    let got = starts(&s, utc(2027, 3, 1, 0), utc(2027, 3, 20, 0));
    // US daylight time starts 2027-03-14
    assert_eq!(
        got,
        [utc(2027, 3, 5, 6), utc(2027, 3, 12, 6), utc(2027, 3, 19, 5)]
    );
}

#[test]
fn until_is_inclusive_and_nothing_follows_it() {
    let s = series(WEEKLY);
    let got = starts(&s, utc(2027, 4, 20, 0), utc(2027, 6, 1, 0));
    assert_eq!(got, [utc(2027, 4, 23, 5), utc(2027, 4, 30, 5)]);
    let all = s.occurrences(utc(2026, 1, 1, 0), utc(2028, 1, 1, 0));
    assert_eq!(
        all.len(),
        27,
        "every Thursday from 2026-10-29 to 2027-04-29"
    );
}

#[test]
fn an_occurrence_still_in_progress_overlaps_the_window() {
    let s = series(WEEKLY);
    // 05:30 UTC on 2026-10-30 is half way through the first meeting
    let got = starts(
        &s,
        Utc.with_ymd_and_hms(2026, 10, 30, 5, 30, 0).unwrap(),
        utc(2026, 10, 31, 0),
    );
    assert_eq!(got, [utc(2026, 10, 30, 5)]);
}

#[test]
fn the_next_occurrence_is_the_first_that_has_not_ended() {
    let s = series(WEEKLY);
    let next = s
        .next_after(utc(2026, 11, 6, 0))
        .expect("a next occurrence");
    assert_eq!(next.start, Moment::Instant(utc(2026, 11, 6, 6)));
    assert!(
        s.next_after(utc(2027, 5, 1, 0)).is_none(),
        "the series has ended"
    );
}

#[test]
fn an_excluded_date_is_skipped() {
    let s = series(&format!(
        "{WEEKLY}EXDATE;TZID=Pacific Standard Time:20261105T220000\r\n"
    ));
    let got = starts(&s, utc(2026, 10, 1, 0), utc(2026, 11, 14, 0));
    assert_eq!(got, [utc(2026, 10, 30, 5), utc(2026, 11, 13, 6)]);
}

#[test]
fn count_stops_the_series() {
    let s = series("RRULE:FREQ=WEEKLY;COUNT=3;BYDAY=TH\r\n");
    let got = starts(&s, utc(2026, 1, 1, 0), utc(2028, 1, 1, 0));
    assert_eq!(
        got,
        [
            utc(2026, 10, 30, 5),
            utc(2026, 11, 6, 6),
            utc(2026, 11, 13, 6)
        ]
    );
}

#[test]
fn an_added_date_is_an_occurrence() {
    let s = series("RRULE:FREQ=WEEKLY;COUNT=2;BYDAY=TH\r\nRDATE:20261120T010000Z\r\n");
    let got = starts(&s, utc(2026, 1, 1, 0), utc(2028, 1, 1, 0));
    assert_eq!(
        got,
        [
            utc(2026, 10, 30, 5),
            utc(2026, 11, 6, 6),
            utc(2026, 11, 20, 1)
        ]
    );
}

/// The organiser moves one meeting to Friday morning, and cancels another.
#[test]
fn an_overridden_occurrence_moves_and_a_cancelled_one_is_gone() {
    let mut s = series(WEEKLY);
    let moved = parse_invite(
        teams("RECURRENCE-ID;TZID=Pacific Standard Time:20261105T220000\r\n")
            .replace(
                "DTSTART;TZID=Pacific Standard Time:20261029T220000",
                "DTSTART;TZID=Pacific Standard Time:20261106T090000",
            )
            .replace(
                "DTEND;TZID=Pacific Standard Time:20261029T230000",
                "DTEND;TZID=Pacific Standard Time:20261106T100000",
            )
            .as_bytes(),
    )
    .expect("the instance parses");
    s.attach(Override::of(&moved).expect("an instance"));
    let cancelled = parse_invite(
        teams("RECURRENCE-ID;TZID=Pacific Standard Time:20261112T220000\r\n")
            .replace("METHOD:REQUEST", "METHOD:CANCEL")
            .as_bytes(),
    )
    .expect("the cancellation parses");
    s.attach(Override::of(&cancelled).expect("an instance"));

    let got = s.occurrences(utc(2026, 10, 1, 0), utc(2026, 11, 21, 0));
    let got: Vec<(Moment, Moment)> = got
        .into_iter()
        .map(|o| (o.start, o.recurrence_id))
        .collect();
    assert_eq!(
        got,
        [
            (
                Moment::Instant(utc(2026, 10, 30, 5)),
                Moment::Instant(utc(2026, 10, 30, 5))
            ),
            // 09:00 PST on the Friday, filed under the Thursday it replaces
            (
                Moment::Instant(utc(2026, 11, 6, 17)),
                Moment::Instant(utc(2026, 11, 6, 6))
            ),
            (
                Moment::Instant(utc(2026, 11, 20, 6)),
                Moment::Instant(utc(2026, 11, 20, 6))
            ),
        ]
    );
}

#[test]
fn a_single_occurrence_invite_is_not_a_series() {
    let one = parse_invite(
        teams(&format!(
            "{WEEKLY}RECURRENCE-ID;TZID=Pacific Standard Time:20261105T220000\r\n"
        ))
        .as_bytes(),
    )
    .expect("parses");
    assert!(Series::of(&one).is_none());
    let plain = parse_invite(teams("").as_bytes()).expect("parses");
    assert!(Series::of(&plain).is_none());
}

#[test]
fn an_all_day_series_repeats_as_dates() {
    let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\nBEGIN:VEVENT\r\n\
               UID:holiday\r\nDTSTAMP:20260101T000000Z\r\nDTSTART;VALUE=DATE:20261103\r\n\
               DTEND;VALUE=DATE:20261104\r\nRRULE:FREQ=YEARLY;COUNT=3\r\nSUMMARY:Culture Day\r\n\
               END:VEVENT\r\nEND:VCALENDAR\r\n";
    let s = Series::of(&parse_invite(ics.as_bytes()).unwrap()).unwrap();
    let got: Vec<Moment> = s
        .occurrences(utc(2026, 1, 1, 0), utc(2030, 1, 1, 0))
        .into_iter()
        .map(|o| o.start)
        .collect();
    let day = |y| Moment::Date(NaiveDate::from_ymd_opt(y, 11, 3).unwrap());
    assert_eq!(got, [day(2026), day(2027), day(2028)]);
}

#[test]
fn a_rule_without_an_end_is_capped() {
    let s = series("RRULE:FREQ=DAILY\r\n");
    let got = s.occurrences(utc(2026, 1, 1, 0), utc(2040, 1, 1, 0));
    assert_eq!(got.len(), mailrs_ical::recur::MAX_OCCURRENCES);
}

#[test]
fn the_rule_reads_in_the_organisers_zone() {
    let s = series(WEEKLY);
    let rule = s.rule().expect("a rule");
    assert_eq!(rule.freq, "WEEKLY");
    assert_eq!(rule.interval, 1);
    assert_eq!(rule.by_day, ["TH"]);
    // 2027-04-30T05:00Z is the evening of the 29th in Santa Clara
    assert_eq!(rule.until, NaiveDate::from_ymd_opt(2027, 4, 29));
    assert_eq!(rule.count, None);
    assert!(rule.plain);
}

#[test]
fn a_series_survives_being_stored() {
    let s = series(&format!(
        "{WEEKLY}EXDATE;TZID=Pacific Standard Time:20261105T220000\r\n"
    ));
    let text = serde_json::to_string(&s).expect("serializes");
    let back: Series = serde_json::from_str(&text).expect("reads back");
    assert_eq!(back, s);
}
