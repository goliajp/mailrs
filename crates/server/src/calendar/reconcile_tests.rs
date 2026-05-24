//! Tests for `reconcile` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

fn parsed(seq: i32, dtstamp: DateTime<Utc>, method: Method) -> ParsedInvite {
    use chrono::TimeZone;
    ParsedInvite {
        method,
        uid: "test-uid".into(),
        sequence: seq,
        dtstamp,
        dtstart: mailrs_ical::CalDateTime::Utc(
            Utc.with_ymd_and_hms(2026, 5, 1, 14, 0, 0).unwrap(),
        ),
        dtend: None,
        duration: None,
        organizer: None,
        attendees: vec![],
        rrule: None,
        exdate: vec![],
        rdate: vec![],
        recurrence_id: None,
        status: None,
        summary: "Test".into(),
        location: None,
        description: None,
        vtimezones: vec![],
    }
}

/// Pure decision logic without DB. Mirrors the apply-rule branch in
/// `reconcile_inbound_invite` so the state machine itself is unit
/// tested independently of sqlx.
fn decide(
    existing_seq: i32,
    existing_dtstamp: Option<DateTime<Utc>>,
    incoming: &ParsedInvite,
) -> bool {
    if incoming.sequence > existing_seq {
        return true;
    }
    if incoming.sequence == existing_seq {
        return match existing_dtstamp {
            Some(prev) => incoming.dtstamp > prev,
            None => true,
        };
    }
    false
}

#[test]
fn higher_sequence_applies() {
    use chrono::TimeZone;
    let now = Utc.with_ymd_and_hms(2026, 4, 30, 0, 0, 0).unwrap();
    assert!(decide(0, Some(now), &parsed(1, now, Method::Update)));
}

#[test]
fn lower_sequence_dropped() {
    use chrono::TimeZone;
    let now = Utc.with_ymd_and_hms(2026, 4, 30, 0, 0, 0).unwrap();
    assert!(!decide(2, Some(now), &parsed(1, now, Method::Cancel)));
}

#[test]
fn same_sequence_newer_dtstamp_applies() {
    use chrono::TimeZone;
    let prev = Utc.with_ymd_and_hms(2026, 4, 30, 10, 0, 0).unwrap();
    let next = Utc.with_ymd_and_hms(2026, 4, 30, 11, 0, 0).unwrap();
    assert!(decide(1, Some(prev), &parsed(1, next, Method::Request)));
}

#[test]
fn same_sequence_older_dtstamp_dropped() {
    use chrono::TimeZone;
    let prev = Utc.with_ymd_and_hms(2026, 4, 30, 11, 0, 0).unwrap();
    let next = Utc.with_ymd_and_hms(2026, 4, 30, 10, 0, 0).unwrap();
    assert!(!decide(1, Some(prev), &parsed(1, next, Method::Request)));
}

#[test]
fn legacy_row_without_dtstamp_accepts_same_sequence() {
    // Pre-MRS-3 rows have NULL dtstamp — treat incoming as authoritative.
    use chrono::TimeZone;
    let now = Utc.with_ymd_and_hms(2026, 4, 30, 0, 0, 0).unwrap();
    assert!(decide(1, None, &parsed(1, now, Method::Update)));
}

#[test]
fn cancel_after_later_update_dropped() {
    // RFC 5546 §3.2.5: CANCEL with stale SEQUENCE must not roll back
    // a more recent UPDATE.
    use chrono::TimeZone;
    let now = Utc.with_ymd_and_hms(2026, 4, 30, 0, 0, 0).unwrap();
    // existing was upgraded to seq=3 via an UPDATE; a re-delivered
    // CANCEL still carrying seq=2 must be ignored.
    assert!(!decide(3, Some(now), &parsed(2, now, Method::Cancel)));
}
