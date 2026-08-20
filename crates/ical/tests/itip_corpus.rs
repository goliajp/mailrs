//! Every invite this corpus holds, read the way production reads one.
//!
//! Five vendors — Google, iCloud, Nextcloud, Outlook, Zoom — times
//! request / update / cancel / recurring. The matrix moved here with
//! `extract_invite_part` in 2026-08: it used to sit inside the server
//! crate that is no longer built, so the only lane exercising these
//! files was the one nobody runs.
//!
//! Per-file expectations come from the filename: `request.eml` is a
//! REQUEST, `update.eml` is a REQUEST with SEQUENCE >= 1, `cancel.eml`
//! is a CANCEL.

#![cfg(feature = "mime")]

use mailrs_ical::mime_part::extract_invite_part;
use mailrs_ical::{CalDateTime, Method, parse_invite};
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

/// Map filename stem → expected iTIP METHOD on the body. `update.eml`
/// wire-format is METHOD=REQUEST + SEQUENCE>=1 (RFC 5546 §3.2.5 — there
/// is no METHOD:UPDATE on the calendar).
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
        let parsed = parse_invite(&extracted.ics_bytes)
            .unwrap_or_else(|e| panic!("{vendor}/{stem}.eml: parse_invite failed: {e:?}"));

        assert!(!parsed.uid.is_empty(), "{vendor}/{stem}.eml UID empty");
        assert_eq!(
            parsed.method,
            expected_method(&stem),
            "{vendor}/{stem}.eml METHOD"
        );
        match &parsed.dtstart {
            CalDateTime::Floating(_) | CalDateTime::Utc(_) => {}
            CalDateTime::Zoned { tz_name, .. } => {
                assert!(!tz_name.is_empty(), "{vendor}/{stem}.eml empty TZID")
            }
            CalDateTime::Date(_) => {}
        }
        assert!(
            !parsed.attendees.is_empty(),
            "{vendor}/{stem}.eml ATTENDEE list empty"
        );
    }
}

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

#[test]
fn cancel_fixtures_carry_cancelled_status() {
    use mailrs_ical::EventStatus;
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
