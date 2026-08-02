//! Document structure and the error paths: multiple VEVENTs, malformed
//! input, and LF-only line endings.

use crate::*;

// =============================================================================
// Multi-VEVENT — only first is taken
// =============================================================================

#[test]
fn multiple_vevent_first_wins() {
    let bytes: &[u8] = b"\
BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\nMETHOD:PUBLISH\r\n\
BEGIN:VEVENT\r\nUID:first\r\nDTSTAMP:19970714T170000Z\r\nDTSTART:19970714T170000Z\r\n\
SUMMARY:First Event\r\nORGANIZER:mailto:o@example.com\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:second\r\nDTSTAMP:19970714T180000Z\r\nDTSTART:19970715T170000Z\r\n\
SUMMARY:Second Event\r\nORGANIZER:mailto:o@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let inv = parse_invite(bytes).expect("parse");
    assert_eq!(inv.uid, "first");
    assert_eq!(inv.summary, "First Event");
}

// =============================================================================
// Error paths
// =============================================================================

#[test]
fn missing_uid_rejected() {
    let bytes: &[u8] = b"\
BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\nMETHOD:REQUEST\r\n\
BEGIN:VEVENT\r\nDTSTAMP:19970714T170000Z\r\nDTSTART:19970714T170000Z\r\n\
ORGANIZER:mailto:o@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    assert!(matches!(
        parse_invite(bytes),
        Err(IcalError::InvalidSemantics(_))
    ));
}

#[test]
fn missing_dtstart_rejected() {
    let bytes: &[u8] = b"\
BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\nMETHOD:REQUEST\r\n\
BEGIN:VEVENT\r\nUID:no-dtstart\r\nDTSTAMP:19970714T170000Z\r\n\
ORGANIZER:mailto:o@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let err = parse_invite(bytes).unwrap_err();
    assert!(matches!(err, IcalError::InvalidSemantics(_)));
}

#[test]
fn missing_dtstamp_rejected() {
    let bytes: &[u8] = b"\
BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\nMETHOD:REQUEST\r\n\
BEGIN:VEVENT\r\nUID:no-dtstamp\r\nDTSTART:19970714T170000Z\r\n\
ORGANIZER:mailto:o@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let err = parse_invite(bytes).unwrap_err();
    assert!(matches!(err, IcalError::InvalidSemantics(_)));
}

// =============================================================================
// LF-only line endings (some legacy senders)
// =============================================================================

#[test]
fn lf_only_line_endings_accepted() {
    let bytes: &[u8] = b"\
BEGIN:VCALENDAR\nVERSION:2.0\nPRODID:-//x//EN\nMETHOD:REQUEST\n\
BEGIN:VEVENT\nUID:lf-only\nDTSTAMP:19970714T170000Z\nDTSTART:19970714T170000Z\n\
ORGANIZER:mailto:o@example.com\nEND:VEVENT\nEND:VCALENDAR\n";
    let inv = parse_invite(bytes).expect("LF-only must parse");
    assert_eq!(inv.uid, "lf-only");
}
