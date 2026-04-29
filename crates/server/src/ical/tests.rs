//! Integration tests for the self-built ical parser.
//!
//! Test corpus lives in the MRS-1 claw at
//! `~/workspace/claws/MRS-1/fixtures/itip/<source>/<method>.ics` and is **not**
//! checked into this repo (per MRS-1 cardinal rule). A small subset of
//! synthesized RFC 5545 §3.4 sample invites is inlined here so that
//! `cargo test -p mailrs-server` is self-contained until MRS-11 promotes
//! fixtures into the repo.

use super::{parse_invite, IcalError};

/// Minimal RFC 5545 §3.4 conformant REQUEST. Hand-typed from the spec to keep
/// this self-contained (no fixture dependency).
const MINIMAL_REQUEST: &[u8] = b"\
BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
PRODID:-//Example Corp//NONSGML Event Calendar//EN\r\n\
METHOD:REQUEST\r\n\
BEGIN:VEVENT\r\n\
UID:guid-1.example.com\r\n\
DTSTAMP:19970714T170000Z\r\n\
ORGANIZER;CN=John Doe:mailto:jdoe@example.com\r\n\
ATTENDEE;RSVP=TRUE;PARTSTAT=NEEDS-ACTION;ROLE=REQ-PARTICIPANT;CN=Jane Smith:mailto:jsmith@example.com\r\n\
DTSTART:19970714T170000Z\r\n\
DTEND:19970715T040000Z\r\n\
SUMMARY:Bastille Day Party\r\n\
SEQUENCE:0\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

#[test]
fn parses_minimal_request_invite() {
    // RED for MRS-2 phase 1: scaffold only. The full implementation lands
    // in the parse + semantics commits and flips this to GREEN.
    let result = parse_invite(MINIMAL_REQUEST);
    assert!(
        result.is_err(),
        "scaffold returns Err until parser is implemented; once implemented, \
         flip this assertion to is_ok() and check fields"
    );
}

#[test]
fn rejects_non_utf8() {
    let bytes: &[u8] = &[0xff, 0xfe, b'B', b'E', b'G', b'I', b'N'];
    assert_eq!(parse_invite(bytes), Err(IcalError::NotUtf8));
}
