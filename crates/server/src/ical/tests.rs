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

use super::{parse_invite, serialize, IcalError, Method, PartStat, Role};
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

/// Vendor consumer matrix. Walks every `.eml` under
/// `tests/fixtures/itip/<vendor>/` through the full inbound pipeline
/// (extract calendar part → parse → semantic typing) and asserts the
/// invariants every wire-shape must satisfy. Per-file expectations are
/// driven by the filename: `request.eml` → REQUEST, `update.eml` →
/// REQUEST with SEQUENCE>=1, `cancel.eml` → CANCEL, etc.
mod consumer_matrix {
    use super::super::{parse_invite, CalDateTime, Method};
    use crate::calendar::invite_extract::extract_invite_part;
    use std::path::PathBuf;

    fn fixtures_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/itip")
    }

    /// Read every `<vendor>/<method>.eml` under the corpus root.
    fn walk_corpus() -> Vec<(String, String, Vec<u8>)> {
        let root = fixtures_root();
        let mut out = Vec::new();
        for vendor_dir in std::fs::read_dir(&root)
            .unwrap_or_else(|e| panic!("read {:?}: {e}", root))
            .filter_map(Result::ok)
        {
            if !vendor_dir.file_type().map(|f| f.is_dir()).unwrap_or(false) {
                continue;
            }
            let vendor = vendor_dir.file_name().to_string_lossy().into_owned();
            for entry in std::fs::read_dir(vendor_dir.path()).unwrap().flatten() {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) != Some("eml") {
                    continue;
                }
                let stem = p.file_stem().unwrap().to_string_lossy().into_owned();
                let bytes = std::fs::read(&p).unwrap();
                out.push((vendor.clone(), stem, bytes));
            }
        }
        assert!(!out.is_empty(), "no fixtures found under {:?}", root);
        out.sort();
        out
    }

    /// Map filename stem → expected iTIP METHOD on the body.
    /// Note `update.eml` wire-format is METHOD=REQUEST + SEQUENCE>=1
    /// (per RFC 5546 §3.2.5 — there is no METHOD:UPDATE on the calendar).
    fn expected_method(stem: &str) -> Method {
        match stem {
            "cancel" => Method::Cancel,
            _ => Method::Request,
        }
    }

    #[test]
    fn every_fixture_extracts_calendar_part() {
        for (vendor, stem, bytes) in walk_corpus() {
            extract_invite_part(&bytes)
                .unwrap_or_else(|| panic!("{vendor}/{stem}.eml: extract_invite_part returned None"));
        }
    }

    #[test]
    fn every_fixture_parses_to_typed_invite() {
        for (vendor, stem, bytes) in walk_corpus() {
            let extracted = extract_invite_part(&bytes)
                .unwrap_or_else(|| panic!("{vendor}/{stem}.eml: no calendar part"));
            let parsed = parse_invite(&extracted.ics_bytes).unwrap_or_else(|e| {
                panic!("{vendor}/{stem}.eml: parse_invite failed: {e:?}")
            });

            assert!(!parsed.uid.is_empty(), "{vendor}/{stem}.eml UID empty");
            assert_eq!(
                parsed.method,
                expected_method(&stem),
                "{vendor}/{stem}.eml METHOD"
            );
            // DTSTART must be set in some form.
            match &parsed.dtstart {
                CalDateTime::Floating(_) | CalDateTime::Utc(_) => {}
                CalDateTime::Zoned { tz_name, .. } => {
                    assert!(!tz_name.is_empty(), "{vendor}/{stem}.eml empty TZID")
                }
                CalDateTime::Date(_) => {}
            }
            // Most fixtures (other than CANCEL retraction) carry attendees.
            // CANCEL spec-wise still includes the attendee list so the
            // recipient can recognize itself; we keep the assertion
            // unconditional to lock that in.
            assert!(
                !parsed.attendees.is_empty(),
                "{vendor}/{stem}.eml ATTENDEE list empty"
            );
        }
    }

    /// UPDATE must carry SEQUENCE >= 1 (per RFC 5546 §3.2.5 — receivers use
    /// SEQUENCE/DTSTAMP monotonicity to discriminate stale vs fresh).
    #[test]
    fn update_fixtures_bump_sequence() {
        for (vendor, stem, bytes) in walk_corpus() {
            if stem != "update" {
                continue;
            }
            let extracted = extract_invite_part(&bytes).unwrap();
            let parsed = parse_invite(&extracted.ics_bytes).unwrap();
            assert!(
                parsed.sequence >= 1,
                "{vendor}/update.eml SEQUENCE={} (expected >= 1)",
                parsed.sequence
            );
        }
    }

    /// CANCEL fixtures must surface STATUS=CANCELLED so the reconcile state
    /// machine (MRS-7) can mark the local row.
    #[test]
    fn cancel_fixtures_carry_cancelled_status() {
        use super::super::EventStatus;
        for (vendor, stem, bytes) in walk_corpus() {
            if stem != "cancel" {
                continue;
            }
            let extracted = extract_invite_part(&bytes).unwrap();
            let parsed = parse_invite(&extracted.ics_bytes).unwrap();
            assert_eq!(
                parsed.status,
                Some(EventStatus::Cancelled),
                "{vendor}/cancel.eml STATUS"
            );
        }
    }

    /// Per-instance override fixtures must carry RECURRENCE-ID so the
    /// MRS-9 partial-index upsert lands on the override row, not the
    /// master series row.
    #[test]
    fn recurring_instance_override_carries_recurrence_id() {
        for (vendor, stem, bytes) in walk_corpus() {
            if stem != "recurring-instance-override" {
                continue;
            }
            let extracted = extract_invite_part(&bytes).unwrap();
            let parsed = parse_invite(&extracted.ics_bytes).unwrap();
            assert!(
                parsed.recurrence_id.is_some(),
                "{vendor}/recurring-instance-override.eml missing RECURRENCE-ID"
            );
        }
    }

    /// Recurring REQUEST fixtures must carry RRULE.
    #[test]
    fn recurring_request_carries_rrule() {
        for (vendor, stem, bytes) in walk_corpus() {
            if stem != "recurring-request" {
                continue;
            }
            let extracted = extract_invite_part(&bytes).unwrap();
            let parsed = parse_invite(&extracted.ics_bytes).unwrap();
            assert!(
                parsed.rrule.is_some(),
                "{vendor}/recurring-request.eml missing RRULE"
            );
        }
    }
}
