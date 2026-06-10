//! `calendar_events` table repo (CRUD + conflict query).
//!
//! Schema lives across migrations 023 (initial) + 031 (iTIP-aware columns).
//! The repo always reads/writes the structured columns; raw `icalendar`
//! TEXT remains the source of truth for re-emit (CalDAV clients GET it
//! verbatim).

// MRS-3 phase: callers wire up incrementally across MRS-3..MRS-9.
// `find_by_uid` / `find_conflicts` / `delete_by_uid` will get callers in
// MRS-5 (conflict API), MRS-6 (RSVP write-back), MRS-7 (state machine).
// Remove this allow once they're all wired.
#![allow(dead_code)]

use sqlx::FromRow;

use crate::pg::BackendPool;
use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, NaiveTime, Timelike, Utc};
use serde_json::json;

use mailrs_ical::vtimezone::{local_to_utc_offset_seconds, resolve};
use mailrs_ical::{
    Attendee as IcalAttendee, CalDateTime, EventStatus, ParsedInvite, PartStat, Person,
    Role as IcalRole, VTimezone,
};

/// Trimmed projection of `calendar_events` for downstream consumers.
/// Full raw icalendar text is fetched separately when needed (CalDAV GET).
#[derive(Debug, Clone)]
pub struct CalendarEventRow {
    pub id: i64,
    pub calendar_id: i64,
    pub uid: String,
    pub etag: String,
    pub summary: String,
    pub dtstart: Option<DateTime<Utc>>,
    pub dtend: Option<DateTime<Utc>>,
    pub organizer: Option<String>,
    pub status: Option<String>,
    pub sequence: i32,
    pub method: Option<String>,
}

/// Insert-or-update a calendar event row from a [`ParsedInvite`].
///
/// Used by:
/// - CalDAV PUT (`web::dav`): client uploads an icalendar object, we parse
///   and project to columns
/// - iTIP RSVP write-back (MRS-6): user accepts an invite, we upsert into
///   the user's own calendar
/// - UPDATE/CANCEL state machine (MRS-7): inbound REQUEST/UPDATE/CANCEL
///   normalised + reconciled
#[derive(sqlx::FromRow)]
struct EventRowSqlx {
    id: i64,
    calendar_id: i64,
    uid: String,
    etag: String,
    summary: String,
    dtstart: Option<DateTime<Utc>>,
    dtend: Option<DateTime<Utc>>,
    organizer: Option<String>,
    status: Option<String>,
    sequence: i32,
    method: Option<String>,
}

/// Same shape as [`EventRowSqlx`] plus `rrule`. Used by `find_conflicts`
/// where the RRULE field decides between static and expansion paths.
#[derive(sqlx::FromRow)]
struct EventRowFull {
    id: i64,
    calendar_id: i64,
    uid: String,
    etag: String,
    summary: String,
    dtstart: Option<DateTime<Utc>>,
    dtend: Option<DateTime<Utc>>,
    organizer: Option<String>,
    status: Option<String>,
    sequence: i32,
    method: Option<String>,
    rrule: Option<String>,
}

impl From<EventRowSqlx> for CalendarEventRow {
    fn from(r: EventRowSqlx) -> Self {
        CalendarEventRow {
            id: r.id,
            calendar_id: r.calendar_id,
            uid: r.uid,
            etag: r.etag,
            summary: r.summary,
            dtstart: r.dtstart,
            dtend: r.dtend,
            organizer: r.organizer,
            status: r.status,
            sequence: r.sequence,
            method: r.method,
        }
    }
}

/// Project a [`CalDateTime`] onto a UTC instant for storage / comparison.
///
/// - `Utc(d)` → `Some(d)`
/// - `Floating(n)` → `Some(n.and_utc())` (best-effort: floating local time
///   has no canonical UTC; we treat it as if the receiver's local zone is
///   UTC. Real-world fixtures rarely use floating for invites.)
/// - `Zoned { tz_name, local }` → resolve via vtimezone (inline blocks
///   first, then chrono-tz / Outlook alias) and convert. Returns None if
///   the tzid is unresolvable.
/// - `Date(d)` → `Some(d at 00:00:00 UTC)` — calendar-day events occupy
///   the full UTC day for conflict purposes (good enough for v1 — refine
///   with VALUE=DATE-aware overlap once MRS-9 lands).
mod convert;
mod queries;
mod upsert;

pub(crate) use convert::caldatetime_to_utc;
pub use queries::{find_by_uid, find_conflicts};
pub use upsert::{delete_by_uid, upsert_from_parsed_invite};
