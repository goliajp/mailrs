//! `upsert_from_parsed_invite` + `delete_by_uid`.

use crate::pg::BackendPool;
use chrono::{DateTime, Datelike, Utc};
use serde_json::json;

use mailrs_ical::{EventStatus, ParsedInvite};

use super::convert::{
    attendee_to_json, caldatetime_to_utc, event_status_str, method_str, person_to_string,
};

pub async fn upsert_from_parsed_invite(
    pool: &BackendPool,
    calendar_id: i64,
    uid: &str,
    parsed: &ParsedInvite,
    raw_icalendar: &str,
    etag: &str,
) -> Result<(), sqlx::Error> {
    let dtstart_utc = caldatetime_to_utc(&parsed.dtstart, &parsed.vtimezones);
    let dtend_utc = parsed
        .dtend
        .as_ref()
        .and_then(|d| caldatetime_to_utc(d, &parsed.vtimezones));
    let recurrence_id_utc = parsed
        .recurrence_id
        .as_ref()
        .and_then(|d| caldatetime_to_utc(d, &parsed.vtimezones));

    let attendees_json = serde_json::Value::Array(
        parsed
            .attendees
            .iter()
            .map(attendee_to_json)
            .collect::<Vec<_>>(),
    );
    let organizer_str = parsed.organizer.as_ref().map(person_to_string);
    let status_str = parsed.status.map(event_status_str);
    let method_str = method_str(parsed.method);

    // Pick the conflict target based on whether this is a master-series
    // upsert (recurrence_id IS NULL — most common, applies to non-
    // recurring events too) or a per-instance override (RFC 5545 §3.8.4.4).
    // Migration 033 set up matching partial unique indexes.
    let sql = if recurrence_id_utc.is_some() {
        "INSERT INTO calendar_events (
            calendar_id, uid, etag, icalendar,
            summary, dtstart, dtend,
            organizer, attendees, sequence, dtstamp,
            status, method, rrule, recurrence_id, last_modified
        ) VALUES (
            $1, $2, $3, $4,
            $5, $6, $7,
            $8, $9, $10, $11,
            $12, $13, $14, $15, $16
        )
        ON CONFLICT (calendar_id, uid, recurrence_id)
        WHERE recurrence_id IS NOT NULL
        DO UPDATE SET
            etag = EXCLUDED.etag,
            icalendar = EXCLUDED.icalendar,
            summary = EXCLUDED.summary,
            dtstart = EXCLUDED.dtstart,
            dtend = EXCLUDED.dtend,
            organizer = EXCLUDED.organizer,
            attendees = EXCLUDED.attendees,
            sequence = EXCLUDED.sequence,
            dtstamp = EXCLUDED.dtstamp,
            status = EXCLUDED.status,
            method = EXCLUDED.method,
            rrule = EXCLUDED.rrule,
            recurrence_id = EXCLUDED.recurrence_id,
            last_modified = EXCLUDED.last_modified,
            updated_at = now()"
    } else {
        "INSERT INTO calendar_events (
            calendar_id, uid, etag, icalendar,
            summary, dtstart, dtend,
            organizer, attendees, sequence, dtstamp,
            status, method, rrule, recurrence_id, last_modified
        ) VALUES (
            $1, $2, $3, $4,
            $5, $6, $7,
            $8, $9, $10, $11,
            $12, $13, $14, $15, $16
        )
        ON CONFLICT (calendar_id, uid)
        WHERE recurrence_id IS NULL
        DO UPDATE SET
            etag = EXCLUDED.etag,
            icalendar = EXCLUDED.icalendar,
            summary = EXCLUDED.summary,
            dtstart = EXCLUDED.dtstart,
            dtend = EXCLUDED.dtend,
            organizer = EXCLUDED.organizer,
            attendees = EXCLUDED.attendees,
            sequence = EXCLUDED.sequence,
            dtstamp = EXCLUDED.dtstamp,
            status = EXCLUDED.status,
            method = EXCLUDED.method,
            rrule = EXCLUDED.rrule,
            recurrence_id = EXCLUDED.recurrence_id,
            last_modified = EXCLUDED.last_modified,
            updated_at = now()"
    };

    sqlx::query(sql)
        .bind(calendar_id)
        .bind(uid)
        .bind(etag)
        .bind(raw_icalendar)
        .bind(&parsed.summary)
        .bind(dtstart_utc)
        .bind(dtend_utc)
        .bind(organizer_str)
        .bind(attendees_json)
        .bind(parsed.sequence)
        .bind(parsed.dtstamp)
        .bind(status_str)
        .bind(method_str)
        .bind(parsed.rrule.as_deref())
        .bind(recurrence_id_utc)
        .bind(parsed.dtstamp)
        .execute(pool)
        .await?;

    Ok(())
}

/// Find calendar events whose [dtstart, dtend) overlaps [start, end).
///
/// Excludes events with status='CANCELLED'. When `exclude_uid` is Some,
/// that UID is filtered out so the caller (e.g. invite card conflict
/// pane) doesn't show the event being looked at as "conflicting with
/// itself".
///
/// Recurring series with `rrule IS NOT NULL` are expanded lazily via the
/// `rrule` crate: the master row's static dtstart may be years in the
/// past, but if the RRULE produces an occurrence inside the query window
/// we still emit a synthetic ConflictRow with the occurrence's dtstart /
/// dtend so the UI shows the right conflict time. Limited to 50 results.
pub async fn delete_by_uid(
    pool: &BackendPool,
    calendar_id: i64,
    uid: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM calendar_events WHERE calendar_id = $1 AND uid = $2")
        .bind(calendar_id)
        .bind(uid)
        .execute(pool)
        .await?;
    Ok(())
}

// ── helpers ──────────────────────────────────────────────────────────
