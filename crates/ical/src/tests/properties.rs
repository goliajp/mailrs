//! Scalar properties: SEQUENCE, METHOD, STATUS, LOCATION, DESCRIPTION,
//! and the case-insensitivity of property names.

use super::fixture;
use crate::*;

// =============================================================================
// SEQUENCE
// =============================================================================

#[test]
fn parses_nonzero_sequence() {
    let bytes = fixture("SEQUENCE:7\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert_eq!(inv.sequence, 7);
}

#[test]
fn sequence_defaults_to_zero() {
    let bytes = fixture("");
    let inv = parse_invite(&bytes).expect("parse");
    assert_eq!(inv.sequence, 0);
}

// =============================================================================
// METHOD
// =============================================================================

#[test]
fn method_missing_defaults_to_publish() {
    let bytes: &[u8] = b"\
BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\n\
BEGIN:VEVENT\r\nUID:no-method\r\nDTSTAMP:19970714T170000Z\r\nDTSTART:19970714T170000Z\r\n\
ORGANIZER:mailto:o@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let inv = parse_invite(bytes).expect("parse");
    assert_eq!(inv.method, Method::Publish);
}

#[test]
fn method_update_outlook_quirk() {
    let bytes: &[u8] = b"\
BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Outlook//EN\r\nMETHOD:UPDATE\r\n\
BEGIN:VEVENT\r\nUID:upd-1\r\nDTSTAMP:19970714T170000Z\r\nDTSTART:19970714T170000Z\r\n\
ORGANIZER:mailto:o@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let inv = parse_invite(bytes).expect("parse");
    assert_eq!(inv.method, Method::Update);
}

#[test]
fn method_lowercase_is_accepted() {
    let bytes: &[u8] = b"\
BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\nMETHOD:reply\r\n\
BEGIN:VEVENT\r\nUID:low\r\nDTSTAMP:19970714T170000Z\r\nDTSTART:19970714T170000Z\r\n\
ORGANIZER:mailto:o@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let inv = parse_invite(bytes).expect("parse");
    assert_eq!(inv.method, Method::Reply);
}

#[test]
fn unknown_method_rejected() {
    let bytes: &[u8] = b"\
BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\nMETHOD:BOGUS\r\n\
BEGIN:VEVENT\r\nUID:b\r\nDTSTAMP:19970714T170000Z\r\nDTSTART:19970714T170000Z\r\n\
ORGANIZER:mailto:o@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    assert!(matches!(
        parse_invite(bytes),
        Err(IcalError::InvalidSemantics(_))
    ));
}

// =============================================================================
// STATUS
// =============================================================================

#[test]
fn status_confirmed() {
    let bytes = fixture("STATUS:CONFIRMED\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert_eq!(inv.status, Some(EventStatus::Confirmed));
}

#[test]
fn status_tentative() {
    let bytes = fixture("STATUS:TENTATIVE\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert_eq!(inv.status, Some(EventStatus::Tentative));
}

#[test]
fn status_cancelled() {
    let bytes = fixture("STATUS:CANCELLED\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert_eq!(inv.status, Some(EventStatus::Cancelled));
}

#[test]
fn status_unknown_rejected() {
    let bytes = fixture("STATUS:SHRUG\r\n");
    assert!(matches!(
        parse_invite(&bytes),
        Err(IcalError::InvalidSemantics(_))
    ));
}

#[test]
fn status_missing_is_none() {
    let bytes = fixture("");
    let inv = parse_invite(&bytes).expect("parse");
    assert!(inv.status.is_none());
}

// =============================================================================
// LOCATION / DESCRIPTION
// =============================================================================

#[test]
fn parses_location() {
    let bytes = fixture("LOCATION:Conference Room A\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert_eq!(inv.location.as_deref(), Some("Conference Room A"));
}

#[test]
fn parses_description_plain() {
    let bytes = fixture("DESCRIPTION:Quarterly review\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert_eq!(inv.description.as_deref(), Some("Quarterly review"));
}

#[test]
fn description_missing_is_none() {
    let bytes = fixture("");
    let inv = parse_invite(&bytes).expect("parse");
    assert!(inv.description.is_none());
}

// =============================================================================
// Property name case insensitivity
// =============================================================================

#[test]
fn lowercase_property_names_accepted() {
    let bytes: &[u8] = b"\
BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\nMETHOD:REQUEST\r\n\
BEGIN:VEVENT\r\nuid:lowercase-props\r\ndtstamp:19970714T170000Z\r\ndtstart:19970714T170000Z\r\n\
summary:Lowercase Test\r\norganizer:mailto:o@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let inv = parse_invite(bytes).expect("parse");
    assert_eq!(inv.uid, "lowercase-props");
    assert_eq!(inv.summary, "Lowercase Test");
}
