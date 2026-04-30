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

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use serde_json::json;
use sqlx::PgPool;

use crate::ical::vtimezone::{local_to_utc_offset_seconds, resolve};
use crate::ical::{
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
pub async fn upsert_from_parsed_invite(
    pool: &PgPool,
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

    sqlx::query(
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
            updated_at = now()",
    )
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
/// Excludes events with status='CANCELLED' (matches the partial index from
/// migration 031). When `exclude_uid` is Some, that UID is filtered out so
/// the caller (e.g. invite card conflict pane) doesn't show the event being
/// looked at as "conflicting with itself".
pub async fn find_conflicts(
    pool: &PgPool,
    calendar_id: i64,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    exclude_uid: Option<&str>,
) -> Result<Vec<CalendarEventRow>, sqlx::Error> {
    let exclude = exclude_uid.unwrap_or("").to_string();
    let rows: Vec<CalendarEventRow> = sqlx::query_as::<_, EventRowSqlx>(
        "SELECT id, calendar_id, uid, etag, summary, dtstart, dtend,
                organizer, status, sequence, method
         FROM calendar_events
         WHERE calendar_id = $1
           AND (status IS DISTINCT FROM 'CANCELLED')
           AND dtstart IS NOT NULL
           AND dtstart < $3
           AND COALESCE(dtend, dtstart) > $2
           AND ($4 = '' OR uid != $4)
         ORDER BY dtstart ASC
         LIMIT 50",
    )
    .bind(calendar_id)
    .bind(start)
    .bind(end)
    .bind(exclude)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(Into::into)
    .collect();

    Ok(rows)
}

/// Lookup a single event by (calendar_id, uid). Returns None when absent.
pub async fn find_by_uid(
    pool: &PgPool,
    calendar_id: i64,
    uid: &str,
) -> Result<Option<CalendarEventRow>, sqlx::Error> {
    let row: Option<EventRowSqlx> = sqlx::query_as(
        "SELECT id, calendar_id, uid, etag, summary, dtstart, dtend,
                organizer, status, sequence, method
         FROM calendar_events
         WHERE calendar_id = $1 AND uid = $2",
    )
    .bind(calendar_id)
    .bind(uid)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(Into::into))
}

/// Hard-delete an event row. CalDAV DELETE handler uses this.
pub async fn delete_by_uid(
    pool: &PgPool,
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
pub(crate) fn caldatetime_to_utc(
    dt: &CalDateTime,
    vtimezones: &[VTimezone],
) -> Option<DateTime<Utc>> {
    match dt {
        CalDateTime::Utc(d) => Some(*d),
        CalDateTime::Floating(n) => Some(n.and_utc()),
        CalDateTime::Zoned { tz_name, local } => {
            let resolved = resolve(tz_name, vtimezones)?;
            let off = local_to_utc_offset_seconds(&resolved, *local)?;
            let utc = local
                .checked_sub_signed(chrono::Duration::seconds(off as i64))?;
            Some(utc.and_utc())
        }
        CalDateTime::Date(d) => Some(naive_date_to_utc_midnight(*d)),
    }
}

fn naive_date_to_utc_midnight(d: NaiveDate) -> DateTime<Utc> {
    let zero = NaiveTime::from_hms_opt(0, 0, 0).expect("00:00:00 always valid");
    NaiveDateTime::new(d, zero).and_utc()
}

fn person_to_string(p: &Person) -> String {
    p.email.clone()
}

fn attendee_to_json(a: &IcalAttendee) -> serde_json::Value {
    json!({
        "email": a.email,
        "cn": a.cn,
        "partstat": partstat_str(a.partstat),
        "role": role_str(a.role),
        "rsvp": a.rsvp,
    })
}

fn partstat_str(p: PartStat) -> &'static str {
    match p {
        PartStat::NeedsAction => "NEEDS-ACTION",
        PartStat::Accepted => "ACCEPTED",
        PartStat::Declined => "DECLINED",
        PartStat::Tentative => "TENTATIVE",
        PartStat::Delegated => "DELEGATED",
        PartStat::Completed => "COMPLETED",
        PartStat::InProcess => "IN-PROCESS",
    }
}

fn role_str(r: IcalRole) -> &'static str {
    match r {
        IcalRole::Chair => "CHAIR",
        IcalRole::ReqParticipant => "REQ-PARTICIPANT",
        IcalRole::OptParticipant => "OPT-PARTICIPANT",
        IcalRole::NonParticipant => "NON-PARTICIPANT",
    }
}

fn event_status_str(s: EventStatus) -> &'static str {
    match s {
        EventStatus::Confirmed => "CONFIRMED",
        EventStatus::Tentative => "TENTATIVE",
        EventStatus::Cancelled => "CANCELLED",
    }
}

fn method_str(m: crate::ical::Method) -> &'static str {
    use crate::ical::Method::*;
    match m {
        Request => "REQUEST",
        Reply => "REPLY",
        Cancel => "CANCEL",
        Update => "UPDATE",
        Counter => "COUNTER",
        Refresh => "REFRESH",
        Add => "ADD",
        Publish => "PUBLISH",
        DeclineCounter => "DECLINECOUNTER",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ical::{Method, ParsedInvite, VTimezone};
    use chrono::TimeZone;

    fn make_parsed(uid: &str, dtstart: CalDateTime) -> ParsedInvite {
        ParsedInvite {
            method: Method::Request,
            uid: uid.into(),
            sequence: 0,
            dtstamp: Utc.with_ymd_and_hms(2026, 4, 30, 0, 0, 0).unwrap(),
            dtstart,
            dtend: None,
            duration: None,
            organizer: None,
            attendees: vec![],
            rrule: None,
            exdate: vec![],
            rdate: vec![],
            recurrence_id: None,
            status: None,
            summary: "test".into(),
            location: None,
            description: None,
            vtimezones: vec![],
        }
    }

    #[test]
    fn utc_caldatetime_passes_through() {
        let d = Utc.with_ymd_and_hms(2026, 5, 1, 14, 0, 0).unwrap();
        let p = make_parsed("a", CalDateTime::Utc(d));
        let out = caldatetime_to_utc(&p.dtstart, &p.vtimezones).unwrap();
        assert_eq!(out, d);
    }

    #[test]
    fn floating_caldatetime_treated_as_utc() {
        let n = chrono::NaiveDate::from_ymd_opt(2026, 5, 1)
            .unwrap()
            .and_hms_opt(14, 0, 0)
            .unwrap();
        let p = make_parsed("b", CalDateTime::Floating(n));
        let out = caldatetime_to_utc(&p.dtstart, &p.vtimezones).unwrap();
        assert_eq!(out, n.and_utc());
    }

    #[test]
    fn zoned_caldatetime_with_iana_resolves() {
        // 2026-05-01 14:00 in Asia/Tokyo = 2026-05-01 05:00 UTC.
        let n = chrono::NaiveDate::from_ymd_opt(2026, 5, 1)
            .unwrap()
            .and_hms_opt(14, 0, 0)
            .unwrap();
        let dt = CalDateTime::Zoned {
            tz_name: "Asia/Tokyo".into(),
            local: n,
        };
        let p = make_parsed("c", dt);
        let out = caldatetime_to_utc(&p.dtstart, &p.vtimezones).unwrap();
        assert_eq!(out, Utc.with_ymd_and_hms(2026, 5, 1, 5, 0, 0).unwrap());
    }

    #[test]
    fn date_only_caldatetime_uses_midnight_utc() {
        let d = chrono::NaiveDate::from_ymd_opt(2026, 5, 1).unwrap();
        let p = make_parsed("d", CalDateTime::Date(d));
        let out = caldatetime_to_utc(&p.dtstart, &p.vtimezones).unwrap();
        assert_eq!(out, Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap());
    }

    #[test]
    fn zoned_unknown_tz_returns_none() {
        let n = chrono::NaiveDate::from_ymd_opt(2026, 5, 1)
            .unwrap()
            .and_hms_opt(14, 0, 0)
            .unwrap();
        let dt = CalDateTime::Zoned {
            tz_name: "Made/Up_Zone_That_Doesn't_Exist".into(),
            local: n,
        };
        let p = make_parsed("e", dt);
        assert!(caldatetime_to_utc(&p.dtstart, &p.vtimezones).is_none());
    }
}
