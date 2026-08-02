//! ATTENDEE and ORGANIZER — PARTSTAT, ROLE, RSVP, and the mailto quirks.

use super::fixture;
use crate::*;

// =============================================================================
// ATTENDEES — PARTSTAT, ROLE, count
// =============================================================================

#[test]
fn parses_three_attendees() {
    let bytes = fixture(
        "ATTENDEE;CN=Alice:mailto:alice@example.com\r\n\
         ATTENDEE;CN=Bob:mailto:bob@example.com\r\n\
         ATTENDEE;CN=Carol:mailto:carol@example.com\r\n",
    );
    let inv = parse_invite(&bytes).expect("parse");
    assert_eq!(inv.attendees.len(), 3);
    let emails: Vec<_> = inv.attendees.iter().map(|a| a.email.as_str()).collect();
    assert_eq!(
        emails,
        vec!["alice@example.com", "bob@example.com", "carol@example.com"]
    );
}

#[test]
fn partstat_accepted() {
    let bytes = fixture("ATTENDEE;PARTSTAT=ACCEPTED:mailto:a@x\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert_eq!(inv.attendees[0].partstat, PartStat::Accepted);
}

#[test]
fn partstat_declined() {
    let bytes = fixture("ATTENDEE;PARTSTAT=DECLINED:mailto:a@x\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert_eq!(inv.attendees[0].partstat, PartStat::Declined);
}

#[test]
fn partstat_tentative() {
    let bytes = fixture("ATTENDEE;PARTSTAT=TENTATIVE:mailto:a@x\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert_eq!(inv.attendees[0].partstat, PartStat::Tentative);
}

#[test]
fn partstat_delegated() {
    let bytes = fixture("ATTENDEE;PARTSTAT=DELEGATED:mailto:a@x\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert_eq!(inv.attendees[0].partstat, PartStat::Delegated);
}

#[test]
fn partstat_unknown_defaults_to_needs_action() {
    let bytes = fixture("ATTENDEE;PARTSTAT=BOGUS:mailto:a@x\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert_eq!(inv.attendees[0].partstat, PartStat::NeedsAction);
}

#[test]
fn partstat_missing_defaults_to_needs_action() {
    let bytes = fixture("ATTENDEE:mailto:a@x\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert_eq!(inv.attendees[0].partstat, PartStat::NeedsAction);
}

#[test]
fn role_chair() {
    let bytes = fixture("ATTENDEE;ROLE=CHAIR:mailto:a@x\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert_eq!(inv.attendees[0].role, Role::Chair);
}

#[test]
fn role_opt_participant() {
    let bytes = fixture("ATTENDEE;ROLE=OPT-PARTICIPANT:mailto:a@x\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert_eq!(inv.attendees[0].role, Role::OptParticipant);
}

#[test]
fn role_non_participant() {
    let bytes = fixture("ATTENDEE;ROLE=NON-PARTICIPANT:mailto:a@x\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert_eq!(inv.attendees[0].role, Role::NonParticipant);
}

#[test]
fn role_unknown_defaults_to_req_participant() {
    let bytes = fixture("ATTENDEE;ROLE=BOGUS:mailto:a@x\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert_eq!(inv.attendees[0].role, Role::ReqParticipant);
}

#[test]
fn rsvp_false_explicit() {
    let bytes = fixture("ATTENDEE;RSVP=FALSE:mailto:a@x\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert!(!inv.attendees[0].rsvp);
}

#[test]
fn rsvp_missing_defaults_false() {
    let bytes = fixture("ATTENDEE:mailto:a@x\r\n");
    let inv = parse_invite(&bytes).expect("parse");
    assert!(!inv.attendees[0].rsvp);
}

// =============================================================================
// ORGANIZER
// =============================================================================

/// Bare email addresses (no `mailto:` prefix) are tolerated for ORGANIZER /
/// ATTENDEE — some buggy producers emit them and rejecting would break
/// real-world invites. Documented in semantics::strip_mailto.
#[test]
fn organizer_without_mailto_tolerated() {
    let bytes: &[u8] = b"\
BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\nMETHOD:REQUEST\r\n\
BEGIN:VEVENT\r\nUID:bare-org\r\nDTSTAMP:19970714T170000Z\r\nDTSTART:19970714T170000Z\r\n\
ORGANIZER:o@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let inv = parse_invite(bytes).expect("bare email should still parse");
    assert_eq!(inv.organizer.as_ref().unwrap().email, "o@example.com");
}

#[test]
fn organizer_uppercase_mailto() {
    let bytes: &[u8] = b"\
BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\nMETHOD:REQUEST\r\n\
BEGIN:VEVENT\r\nUID:upper\r\nDTSTAMP:19970714T170000Z\r\nDTSTART:19970714T170000Z\r\n\
ORGANIZER:MAILTO:o@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let inv = parse_invite(bytes).expect("parse");
    assert_eq!(inv.organizer.as_ref().unwrap().email, "o@example.com");
}
