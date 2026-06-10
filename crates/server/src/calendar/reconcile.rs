//! State-machine reconciliation per RFC 5546 §3.2.
//!
//! When a later-version invite (UPDATE / CANCEL / a higher-SEQUENCE
//! REQUEST — Outlook/Exchange typically use the last form for updates)
//! arrives for an event the user already has on their own calendar,
//! decide whether to apply or drop based on SEQUENCE + DTSTAMP.
//!
//! Rules (matching `goals.md` Decisions and the MRS-7 ticket spec):
//! - incoming SEQUENCE > existing → apply
//! - incoming SEQUENCE == existing && DTSTAMP newer → apply (organizer
//!   re-emitted same revision)
//! - incoming SEQUENCE < existing → drop (stale; CANCEL after a later
//!   UPDATE must not undo the UPDATE — RFC 5546 §3.2.5)
//! - method=CANCEL with apply-decision → set status=CANCELLED, keep row
//!   for audit
//! - any other method with apply-decision → upsert content
//!
//! Events the user has not RSVP'd (no row in their calendar) are not
//! reconciled. The invite_payload on the message itself is always kept
//! current via MRS-4's post-store update, so the web invite-card UI sees
//! the latest version regardless.

use crate::pg::BackendPool;
use chrono::{DateTime, Utc};

use mailrs_ical::{Method, ParsedInvite};

#[derive(Debug, PartialEq, Eq)]
pub enum ReconcileOutcome {
    /// User has not RSVP'd this event — calendar untouched.
    NotInCalendar,
    /// Incoming invite is older than what's already on the calendar — drop.
    Stale {
        existing_sequence: i32,
        incoming_sequence: i32,
    },
    /// Calendar row was updated.
    Applied { method: String, was_cancelled: bool },
}

pub async fn reconcile_inbound_invite(
    pool: &BackendPool,
    user: &str,
    parsed: &ParsedInvite,
    raw_icalendar: &str,
) -> Result<ReconcileOutcome, sqlx::Error> {
    // The user's default calendar is the only one mailrs writes to today;
    // future MRS phases may expose multi-calendar selection.
    let cal_id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM calendars
         WHERE account_address = $1
         ORDER BY id ASC LIMIT 1",
    )
    .bind(user)
    .fetch_optional(pool)
    .await?;
    let Some(cal_id) = cal_id else {
        return Ok(ReconcileOutcome::NotInCalendar);
    };

    let existing: Option<(i32, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT sequence, dtstamp
         FROM calendar_events
         WHERE calendar_id = $1 AND uid = $2",
    )
    .bind(cal_id)
    .bind(&parsed.uid)
    .fetch_optional(pool)
    .await?;

    let Some((existing_seq, existing_dtstamp)) = existing else {
        return Ok(ReconcileOutcome::NotInCalendar);
    };

    let apply = if parsed.sequence > existing_seq {
        true
    } else if parsed.sequence == existing_seq {
        // Same sequence: trust DTSTAMP. If we don't have one stored
        // (legacy row), accept the incoming as authoritative.
        match existing_dtstamp {
            Some(prev) => parsed.dtstamp > prev,
            None => true,
        }
    } else {
        false
    };

    if !apply {
        return Ok(ReconcileOutcome::Stale {
            existing_sequence: existing_seq,
            incoming_sequence: parsed.sequence,
        });
    }

    let was_cancelled = matches!(parsed.method, Method::Cancel);

    if was_cancelled {
        sqlx::query(
            "UPDATE calendar_events
             SET status = 'CANCELLED',
                 sequence = $1,
                 dtstamp = $2,
                 last_modified = $2,
                 method = 'CANCEL',
                 updated_at = now()
             WHERE calendar_id = $3 AND uid = $4",
        )
        .bind(parsed.sequence)
        .bind(parsed.dtstamp)
        .bind(cal_id)
        .bind(&parsed.uid)
        .execute(pool)
        .await?;
    } else {
        // REQUEST/UPDATE/etc with newer revision: replace content via the
        // structured upsert. Etag derived from DTSTAMP so the same
        // organizer-side revision yields a stable etag (CalDAV clients
        // pick up updates via etag mismatch).
        let etag = format!("{:x}", parsed.dtstamp.timestamp_micros());
        super::event::upsert_from_parsed_invite(
            pool,
            cal_id,
            &parsed.uid,
            parsed,
            raw_icalendar,
            &etag,
        )
        .await?;
    }

    Ok(ReconcileOutcome::Applied {
        method: format!("{:?}", parsed.method).to_uppercase(),
        was_cancelled,
    })
}

#[cfg(test)]
mod tests {
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
}
