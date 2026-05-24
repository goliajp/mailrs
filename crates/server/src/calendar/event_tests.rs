//! Tests for `event` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;
use mailrs_ical::{Method, ParsedInvite, VTimezone};
use chrono::TimeZone;

fn make_parsed(uid: &str, dtstart: CalDateTime) -> ParsedInvite {
    ParsedInvite {
        method: Method::Request,
        uid: uid.into(),
        sequence: 0,
        dtstamp: Utc.with_ymd_and_hms(2026, 4, 30, 0, 0, 0).unwrap(),
        dtstart,
        dtend: None,
        duration: None,
        organizer: None,
        attendees: vec![],
        rrule: None,
        exdate: vec![],
        rdate: vec![],
        recurrence_id: None,
        status: None,
        summary: "test".into(),
        location: None,
        description: None,
        vtimezones: vec![],
    }
}

#[test]
fn utc_caldatetime_passes_through() {
    let d = Utc.with_ymd_and_hms(2026, 5, 1, 14, 0, 0).unwrap();
    let p = make_parsed("a", CalDateTime::Utc(d));
    let out = caldatetime_to_utc(&p.dtstart, &p.vtimezones).unwrap();
    assert_eq!(out, d);
}

#[test]
fn floating_caldatetime_treated_as_utc() {
    let n = chrono::NaiveDate::from_ymd_opt(2026, 5, 1)
        .unwrap()
        .and_hms_opt(14, 0, 0)
        .unwrap();
    let p = make_parsed("b", CalDateTime::Floating(n));
    let out = caldatetime_to_utc(&p.dtstart, &p.vtimezones).unwrap();
    assert_eq!(out, n.and_utc());
}

#[test]
fn zoned_caldatetime_with_iana_resolves() {
    // 2026-05-01 14:00 in Asia/Tokyo = 2026-05-01 05:00 UTC.
    let n = chrono::NaiveDate::from_ymd_opt(2026, 5, 1)
        .unwrap()
        .and_hms_opt(14, 0, 0)
        .unwrap();
    let dt = CalDateTime::Zoned {
        tz_name: "Asia/Tokyo".into(),
        local: n,
    };
    let p = make_parsed("c", dt);
    let out = caldatetime_to_utc(&p.dtstart, &p.vtimezones).unwrap();
    assert_eq!(out, Utc.with_ymd_and_hms(2026, 5, 1, 5, 0, 0).unwrap());
}

#[test]
fn date_only_caldatetime_uses_midnight_utc() {
    let d = chrono::NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
    let p = make_parsed("d", CalDateTime::Date(d));
    let out = caldatetime_to_utc(&p.dtstart, &p.vtimezones).unwrap();
    assert_eq!(out, Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap());
}

#[test]
fn zoned_unknown_tz_returns_none() {
    let n = chrono::NaiveDate::from_ymd_opt(2026, 5, 1)
        .unwrap()
        .and_hms_opt(14, 0, 0)
        .unwrap();
    let dt = CalDateTime::Zoned {
        tz_name: "Made/Up_Zone_That_Doesn't_Exist".into(),
        local: n,
    };
    let p = make_parsed("e", dt);
    assert!(caldatetime_to_utc(&p.dtstart, &p.vtimezones).is_none());
}
