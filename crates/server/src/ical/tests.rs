//! Integration tests for the self-built ical parser.
//!
//! Test corpus lives in the MRS-1 claw at
//! `~/workspace/claws/MRS-1/fixtures/itip/<source>/<method>.ics` and is **not**
//! checked into this repo (per MRS-1 cardinal rule). A small subset of
//! synthesized RFC 5545 §3.4 sample invites is inlined here so that
//! `cargo test -p mailrs-server` is self-contained until MRS-11 promotes
//! fixtures into the repo.

use super::{parse_invite, IcalError, Method, PartStat, Role};
use chrono::TimeZone;

/// Minimal RFC 5545 §3.4-derived REQUEST. Hand-typed from the spec to keep
/// this self-contained (no fixture dependency). Adjusted to include
/// SEQUENCE + ATTENDEE so the round-trip exercises the iTIP-relevant fields.
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
    let invite = parse_invite(MINIMAL_REQUEST).expect("minimal REQUEST should parse");

    assert_eq!(invite.method, Method::Request);
    assert_eq!(invite.uid, "guid-1.example.com");
    assert_eq!(invite.sequence, 0);
    assert_eq!(invite.summary, "Bastille Day Party");

    let organizer = invite.organizer.as_ref().expect("organizer present");
    assert_eq!(organizer.email, "jdoe@example.com");
    assert_eq!(organizer.cn.as_deref(), Some("John Doe"));

    assert_eq!(invite.attendees.len(), 1);
    let att = &invite.attendees[0];
    assert_eq!(att.email, "jsmith@example.com");
    assert_eq!(att.cn.as_deref(), Some("Jane Smith"));
    assert_eq!(att.partstat, PartStat::NeedsAction);
    assert_eq!(att.role, Role::ReqParticipant);
    assert!(att.rsvp);

    // DTSTAMP / DTSTART / DTEND are UTC in this fixture
    let expected_dtstamp = chrono::Utc.with_ymd_and_hms(1997, 7, 14, 17, 0, 0).unwrap();
    assert_eq!(invite.dtstamp, expected_dtstamp);
}

#[test]
fn rejects_non_utf8() {
    let bytes: &[u8] = &[0xff, 0xfe, b'B', b'E', b'G', b'I', b'N'];
    assert_eq!(parse_invite(bytes), Err(IcalError::NotUtf8));
}

#[test]
fn rejects_no_vcalendar() {
    let bytes = b"hello world\r\n";
    assert!(matches!(
        parse_invite(bytes),
        Err(IcalError::InvalidSyntax(_))
    ));
}

#[test]
fn rejects_no_event() {
    let bytes: &[u8] = b"\
BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
PRODID:-//Example//EN\r\n\
END:VCALENDAR\r\n";
    assert_eq!(parse_invite(bytes), Err(IcalError::NoEvent));
}

/// Per RFC 5545 §3.1, a long line must be folded with CRLF + WSP. The parser
/// must unfold it back so SUMMARY / DESCRIPTION etc. survive long values.
#[test]
fn unfolds_continuation_lines() {
    let bytes: &[u8] = b"\
BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
PRODID:-//Example//EN\r\n\
METHOD:REQUEST\r\n\
BEGIN:VEVENT\r\n\
UID:fold-test\r\n\
DTSTAMP:19970714T170000Z\r\n\
DTSTART:19970714T170000Z\r\n\
SUMMARY:This summary\r\n  is folded\r\n  across three lines\r\n\
ORGANIZER:mailto:o@example.com\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";
    let invite = parse_invite(bytes).expect("should parse");
    assert_eq!(invite.summary, "This summary is folded across three lines");
}

/// METHOD recognition for all RFC 5546 values mailrs cares about.
#[test]
fn recognises_itip_methods() {
    for (text_method, expected) in [
        ("REQUEST", Method::Request),
        ("REPLY", Method::Reply),
        ("CANCEL", Method::Cancel),
        ("PUBLISH", Method::Publish),
        ("ADD", Method::Add),
        ("REFRESH", Method::Refresh),
        ("COUNTER", Method::Counter),
        ("DECLINECOUNTER", Method::DeclineCounter),
    ] {
        let body = format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\nMETHOD:{text_method}\r\n\
             BEGIN:VEVENT\r\nUID:m\r\nDTSTAMP:19970714T170000Z\r\nDTSTART:19970714T170000Z\r\n\
             ORGANIZER:mailto:o@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
        );
        let parsed = parse_invite(body.as_bytes())
            .unwrap_or_else(|e| panic!("failed to parse METHOD={text_method}: {e:?}"));
        assert_eq!(parsed.method, expected, "METHOD={text_method}");
    }
}
