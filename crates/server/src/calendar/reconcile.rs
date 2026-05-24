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

use chrono::{DateTime, Utc};
use sqlx::PgPool;

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
    pool: &PgPool,
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
#[path = "reconcile_tests.rs"]
mod tests;
