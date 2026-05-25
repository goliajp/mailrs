//! Integration tests for the self-built ical parser.
//!
//! Two corpora:
//! - **inlined RFC 5545 §3.4 samples** (this file) — minimal hand-typed
//!   invites that round-trip every public field, run on `cargo test` with
//!   no I/O.
//! - **vendor consumer matrix** (`consumer_matrix` submodule) — promoted from
//!   the MRS-1 claw at MRS-11 land time. `tests/fixtures/itip/<vendor>/*.eml`
//!   walks Outlook / Google / iCloud / Zoom / Nextcloud wire shapes through
//!   `invite_extract` + `parse_invite` to lock in the wire-format quirks
//!   each vendor ships.

use super::{CalDateTime, EventStatus, IcalError, Method, PartStat, Role, parse_invite, serialize};
use chrono::TimeZone;

/// Build a minimal VEVENT-bearing VCALENDAR with the given inner body. The
/// fixture provides VERSION + PRODID + METHOD + UID + DTSTAMP + DTSTART so
/// each focused test can append the property it actually exercises.
fn fixture(extra: &str) -> Vec<u8> {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\nMETHOD:REQUEST\r\n\
         BEGIN:VEVENT\r\nUID:t-{}\r\nDTSTAMP:19970714T170000Z\r\nDTSTART:19970714T170000Z\r\n\
         ORGANIZER:mailto:o@example.com\r\n\
         {extra}\
         END:VEVENT\r\nEND:VCALENDAR\r\n",
        rand_uid()
    )
    .into_bytes()
}

fn rand_uid() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("t{n}@example")
}

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

/// `parse → serialize → parse` must yield a semantically-equal invite.
/// Byte-for-byte round-trip is intentionally not asserted: serialization
/// re-canonicalizes parameter case, ATTENDEE param order, etc.
#[test]
fn round_trip_minimal_request() {
    let invite = parse_invite(MINIMAL_REQUEST).expect("parse");
    let serialized = serialize::serialize(&invite).expect("serialize");
    let reparsed = parse_invite(serialized.as_bytes())
        .unwrap_or_else(|e| panic!("re-parse failed: {e:?}\n--- text ---\n{serialized}"));
    assert_eq!(invite, reparsed, "semantic round-trip");
}

/// Round-trip survives TEXT escapes (commas, semicolons, newlines, backslash).
#[test]
fn round_trip_escapes_text_fields() {
    let bytes: &[u8] = b"\
BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
PRODID:-//x//EN\r\n\
METHOD:REQUEST\r\n\
BEGIN:VEVENT\r\n\
UID:esc-test\r\n\
DTSTAMP:19970714T170000Z\r\n\
DTSTART:19970714T170000Z\r\n\
SUMMARY:Hello\\, world\\; meeting\r\n\
DESCRIPTION:line1\\nline2\\\\done\r\n\
ORGANIZER:mailto:o@example.com\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";
    let invite = parse_invite(bytes).expect("parse");
    assert_eq!(invite.summary, "Hello, world; meeting");
    assert_eq!(invite.description.as_deref(), Some("line1\nline2\\done"));

    let serialized = serialize::serialize(&invite).expect("serialize");
    let reparsed = parse_invite(serialized.as_bytes()).expect("re-parse");
    assert_eq!(invite, reparsed);
}

/// Long lines must be folded at 75 octets per §3.1 and unfold cleanly on
/// re-parse.
#[test]
fn round_trip_folds_and_unfolds_long_summary() {
    let long_summary = "x".repeat(200);
    let body = format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\nMETHOD:REQUEST\r\n\
         BEGIN:VEVENT\r\nUID:long-test\r\nDTSTAMP:19970714T170000Z\r\nDTSTART:19970714T170000Z\r\n\
         SUMMARY:{long_summary}\r\nORGANIZER:mailto:o@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    );
    let invite = parse_invite(body.as_bytes()).expect("parse");
    assert_eq!(invite.summary, long_summary);

    let serialized = serialize::serialize(&invite).expect("serialize");
    // Every physical line must respect the 75-octet limit.
    for line in serialized.split_terminator("\r\n") {
        let body = line.strip_prefix(' ').unwrap_or(line);
        assert!(
            body.len() <= 75,
            "physical line over 75 octets: {} chars",
            body.len()
        );
    }
    let reparsed = parse_invite(serialized.as_bytes()).expect("re-parse");
    assert_eq!(invite.summary, reparsed.summary);
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
