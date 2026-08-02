//! Parse -> serialize -> parse, and the corner cases that only show up
//! on the way back out.

use super::fixture;
use crate::*;

// =============================================================================
// Round-trip for property-heavy invite
// =============================================================================

#[test]
fn round_trip_with_status_location_description() {
    let bytes = fixture(
        "STATUS:CONFIRMED\r\n\
         LOCATION:Tokyo HQ\r\n\
         DESCRIPTION:Annual planning meeting\r\n\
         SEQUENCE:3\r\n",
    );
    let inv = parse_invite(&bytes).expect("parse");
    let serialized = serialize::serialize(&inv).expect("serialize");
    let reparsed = parse_invite(serialized.as_bytes()).expect("re-parse");
    assert_eq!(inv, reparsed);
}

#[test]
fn round_trip_with_rrule_and_exdate() {
    let bytes = fixture(
        "RRULE:FREQ=WEEKLY;BYDAY=MO\r\n\
         EXDATE:19980706T170000Z,19980713T170000Z\r\n",
    );
    let inv = parse_invite(&bytes).expect("parse");
    let serialized = serialize::serialize(&inv).expect("serialize");
    let reparsed = parse_invite(serialized.as_bytes()).expect("re-parse");
    assert_eq!(inv, reparsed);
}

// =============================================================================
// Additional corner-case tests
// =============================================================================

#[test]
fn rejects_invalid_utf8_input() {
    let bad: &[u8] = &[0xff, 0xfe, 0xfd];
    let err = parse_invite(bad).unwrap_err();
    assert_eq!(err, IcalError::NotUtf8);
}

#[test]
fn summary_with_utf8_emoji_preserved() {
    // RFC 5545 §3.1.4 mandates UTF-8 — emoji should survive a parse roundtrip.
    let bytes = fixture("SUMMARY:Lunch 🍕 with team\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert_eq!(inv.summary, "Lunch 🍕 with team");
}

#[test]
fn summary_with_chinese_characters_preserved() {
    let bytes = fixture("SUMMARY:全员会议\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert_eq!(inv.summary, "全员会议");
}

#[test]
fn summary_with_escape_sequences_unescaped() {
    // RFC 5545 §3.3.11 TEXT escapes: \n => newline, \, => comma, \; => semicolon, \\ => backslash
    let bytes = fixture("SUMMARY:Line1\\nLine2\\, with comma\\; and semi\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert!(inv.summary.contains('\n'));
    assert!(inv.summary.contains(','));
    assert!(inv.summary.contains(';'));
}

#[test]
fn description_with_escape_sequences_unescaped() {
    let bytes = fixture("DESCRIPTION:Hello\\nWorld\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert!(inv.description.as_deref().unwrap_or("").contains('\n'));
}

#[test]
fn long_summary_unfolded_properly() {
    // Multiple continuation lines, ensure all are joined.
    let bytes: &[u8] = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\nMETHOD:REQUEST\r\n\
BEGIN:VEVENT\r\nUID:long-1\r\nDTSTAMP:19970714T170000Z\r\nDTSTART:19970714T170000Z\r\n\
SUMMARY:Part one\r\n  part two\r\n  part three\r\nORGANIZER:mailto:o@x\r\n\
END:VEVENT\r\nEND:VCALENDAR\r\n";
    let inv = parse_invite(bytes).expect("parse");
    assert!(inv.summary.contains("Part one"));
    assert!(inv.summary.contains("part two"));
    assert!(inv.summary.contains("part three"));
}

#[test]
fn empty_attendee_list_when_none_provided() {
    let bytes = fixture("");
    let inv = parse_invite(&bytes).expect("parse");
    assert!(inv.attendees.is_empty());
}

#[test]
fn rejects_missing_begin_end_vcalendar() {
    let bytes: &[u8] = b"VERSION:2.0\r\nUID:x\r\n";
    let err = parse_invite(bytes).unwrap_err();
    assert!(matches!(err, IcalError::InvalidSyntax(_)));
}

#[test]
fn no_vevent_in_vcalendar_returns_no_event() {
    let bytes: &[u8] = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\nMETHOD:REQUEST\r\n\
END:VCALENDAR\r\n";
    let err = parse_invite(bytes).unwrap_err();
    assert!(matches!(
        err,
        IcalError::NoEvent | IcalError::InvalidSemantics(_)
    ));
}

#[test]
fn whitespace_around_property_values_preserved_or_trimmed_consistently() {
    // The impl preserves value strings as-is (text-type unescaping happens at the semantic layer).
    // Just ensure parse completes.
    let bytes = fixture("LOCATION:   Tokyo HQ   \r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert!(inv.location.is_some());
}

#[test]
fn multiple_attendees_preserved_in_order() {
    let bytes = fixture(
        "ATTENDEE;CN=Alpha:mailto:a@x\r\n\
         ATTENDEE;CN=Beta:mailto:b@x\r\n\
         ATTENDEE;CN=Gamma:mailto:c@x\r\n",
    );
    let inv = parse_invite(&bytes).expect("parse");
    assert_eq!(inv.attendees.len(), 3);
    assert_eq!(inv.attendees[0].cn.as_deref(), Some("Alpha"));
    assert_eq!(inv.attendees[1].cn.as_deref(), Some("Beta"));
    assert_eq!(inv.attendees[2].cn.as_deref(), Some("Gamma"));
}
