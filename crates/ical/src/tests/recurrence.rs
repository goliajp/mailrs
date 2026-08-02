//! Recurrence and time: RRULE / EXDATE / RDATE / RECURRENCE-ID,
//! the CalDateTime variants, DTEND vs DURATION, and VTIMEZONE.

use super::fixture;
use crate::*;
use chrono::TimeZone;

// =============================================================================
// RRULE / EXDATE / RDATE / RECURRENCE-ID
// =============================================================================

#[test]
fn rrule_captured_raw() {
    let bytes = fixture("RRULE:FREQ=WEEKLY;BYDAY=MO,WE,FR;COUNT=10\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert_eq!(
        inv.rrule.as_deref(),
        Some("FREQ=WEEKLY;BYDAY=MO,WE,FR;COUNT=10")
    );
}

#[test]
fn rrule_missing_is_none() {
    let bytes = fixture("");
    let inv = parse_invite(&bytes).expect("parse");
    assert!(inv.rrule.is_none());
}

#[test]
fn exdate_single_utc() {
    let bytes = fixture("EXDATE:19980402T170000Z\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert_eq!(inv.exdate.len(), 1);
    match &inv.exdate[0] {
        CalDateTime::Utc(dt) => {
            assert_eq!(
                *dt,
                chrono::Utc.with_ymd_and_hms(1998, 4, 2, 17, 0, 0).unwrap()
            );
        }
        other => panic!("expected Utc, got {other:?}"),
    }
}

#[test]
fn exdate_multiple_comma_separated() {
    let bytes = fixture("EXDATE:19980402T170000Z,19980409T170000Z,19980416T170000Z\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert_eq!(inv.exdate.len(), 3);
}

#[test]
fn rdate_single_utc() {
    let bytes = fixture("RDATE:19980501T170000Z\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert_eq!(inv.rdate.len(), 1);
}

#[test]
fn recurrence_id_present() {
    let bytes = fixture("RECURRENCE-ID:19980402T170000Z\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert!(inv.recurrence_id.is_some());
}

#[test]
fn recurrence_id_missing_is_none() {
    let bytes = fixture("");
    let inv = parse_invite(&bytes).expect("parse");
    assert!(inv.recurrence_id.is_none());
}

// =============================================================================
// CalDateTime variants
// =============================================================================

#[test]
fn dtstart_with_tzid_is_zoned() {
    let bytes: &[u8] = b"\
BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\nMETHOD:REQUEST\r\n\
BEGIN:VEVENT\r\nUID:tz-1\r\nDTSTAMP:19970714T170000Z\r\n\
DTSTART;TZID=America/New_York:19980119T020000\r\n\
ORGANIZER:mailto:o@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let inv = parse_invite(bytes).expect("parse");
    match &inv.dtstart {
        CalDateTime::Zoned { tz_name, .. } => assert_eq!(tz_name, "America/New_York"),
        other => panic!("expected Zoned, got {other:?}"),
    }
}

#[test]
fn dtstart_date_only() {
    let bytes: &[u8] = b"\
BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\nMETHOD:REQUEST\r\n\
BEGIN:VEVENT\r\nUID:date-1\r\nDTSTAMP:19970714T170000Z\r\n\
DTSTART;VALUE=DATE:19980118\r\n\
ORGANIZER:mailto:o@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let inv = parse_invite(bytes).expect("parse");
    match &inv.dtstart {
        CalDateTime::Date(d) => {
            assert_eq!(*d, chrono::NaiveDate::from_ymd_opt(1998, 1, 18).unwrap());
        }
        other => panic!("expected Date, got {other:?}"),
    }
}

#[test]
fn dtstart_floating() {
    let bytes: &[u8] = b"\
BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\nMETHOD:REQUEST\r\n\
BEGIN:VEVENT\r\nUID:flo-1\r\nDTSTAMP:19970714T170000Z\r\n\
DTSTART:19980118T230000\r\n\
ORGANIZER:mailto:o@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let inv = parse_invite(bytes).expect("parse");
    assert!(matches!(inv.dtstart, CalDateTime::Floating(_)));
}

// =============================================================================
// DTEND vs DURATION
// =============================================================================

#[test]
fn duration_in_lieu_of_dtend() {
    let bytes: &[u8] = b"\
BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\nMETHOD:REQUEST\r\n\
BEGIN:VEVENT\r\nUID:dur-1\r\nDTSTAMP:19970714T170000Z\r\nDTSTART:19970714T170000Z\r\n\
DURATION:PT1H30M\r\n\
ORGANIZER:mailto:o@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let inv = parse_invite(bytes).expect("parse");
    assert!(inv.dtend.is_none());
    assert_eq!(inv.duration.unwrap().num_minutes(), 90);
}

// =============================================================================
// VTIMEZONE captured in VCALENDAR
// =============================================================================

#[test]
fn vtimezone_captured_into_invite() {
    let bytes: &[u8] = b"\
BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\nMETHOD:REQUEST\r\n\
BEGIN:VTIMEZONE\r\nTZID:America/New_York\r\n\
BEGIN:STANDARD\r\nDTSTART:19701101T020000\r\nTZOFFSETFROM:-0400\r\nTZOFFSETTO:-0500\r\nEND:STANDARD\r\n\
END:VTIMEZONE\r\n\
BEGIN:VEVENT\r\nUID:tz-vev\r\nDTSTAMP:19970714T170000Z\r\n\
DTSTART;TZID=America/New_York:19980119T020000\r\n\
ORGANIZER:mailto:o@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let inv = parse_invite(bytes).expect("parse");
    assert_eq!(inv.vtimezones.len(), 1);
    assert_eq!(inv.vtimezones[0].tzid, "America/New_York");
}
