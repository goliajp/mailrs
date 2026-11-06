//! `/api/calendar/*` endpoints — feeds CRUD, events and conflicts.
//!
//! Storage on network kevy:
//!
//! ```text
//! calendar_feeds:<user>          hash { feed_id -> JSON CalendarFeed }
//! calendar_events:<user>         zset  score = dtstart_epoch, member = event_uid
//! calendar_event:<user>:<uid>    hash  { summary, dtstart, dtend, organizer, status, source, partstat, recurrence, … }
//! ```
//!
//! A recurring event is one row holding its series; the events and
//! conflicts readers expand it into the occurrences of the window asked
//! for (`calendar_events::series`), each with its `recurrence_id`.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Query, State};
use axum::http::StatusCode;
use mailrs_core_sidestate::families::calendar_events::span;
use serde::{Deserialize, Serialize};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

mod feeds;
pub use feeds::{CreateFeedRequest, FeedView, FeedWire, create_feed, delete_feed, list_feeds};

#[derive(Debug, Deserialize)]
pub struct ConflictsQuery {
    /// ISO-8601 UTC start of the window.
    pub start: String,
    /// ISO-8601 UTC end of the window.
    pub end: String,
    /// Optional event UID to exclude (the one being RSVP'd).
    #[serde(default)]
    pub exclude_uid: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct EventRow {
    pub uid: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub dtstart: Option<String>,
    #[serde(default)]
    pub dtend: Option<String>,
    #[serde(default)]
    pub organizer: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    /// The reader's answer — `ACCEPTED` / `TENTATIVE` / `DECLINED` —
    /// absent when they have not given one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partstat: Option<String>,
    /// The Message-ID of the mail the invitation came in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// The conversation that mail is in now, for the calendar to open:
    /// looked up when the events are listed, since threading can move it.
    /// Absent when the mail is gone or the event did not come by mail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    /// For one occurrence of a recurring event, which occurrence it is (the
    /// start the rule gave it, RFC 3339, or a date for an all-day series);
    /// the same `uid` comes back once per occurrence in the window. Absent
    /// for an event that does not repeat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recurrence_id: Option<String>,
}

fn parse_iso(s: &str) -> Option<i64> {
    // Accept `YYYY-MM-DDTHH:MM:SSZ` and `+00:00`. Full parser lives in
    // chrono; for the fastcore port we handle the common shapes with a
    // small stateful scanner to avoid pulling chrono into webapi's dep
    // graph.
    let bytes = s.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let year: i64 = std::str::from_utf8(&bytes[0..4]).ok()?.parse().ok()?;
    let mon: i64 = std::str::from_utf8(&bytes[5..7]).ok()?.parse().ok()?;
    let day: i64 = std::str::from_utf8(&bytes[8..10]).ok()?.parse().ok()?;
    let hour: i64 = std::str::from_utf8(&bytes[11..13]).ok()?.parse().ok()?;
    let min: i64 = std::str::from_utf8(&bytes[14..16]).ok()?.parse().ok()?;
    let sec: i64 = std::str::from_utf8(&bytes[17..19]).ok()?.parse().ok()?;
    // Very small Gregorian → epoch. Sufficient for ordering / conflict.
    let mut days: i64 = 0;
    for y in 1970..year {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        days += if leap { 366 } else { 365 };
    }
    let ml = [31i64, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    for m in 1..mon {
        let base = ml[(m - 1) as usize];
        days += if m == 2 && leap { 29 } else { base };
    }
    days += day - 1;
    Some(days * 86_400 + hour * 3600 + min * 60 + sec)
}

/// GET /api/calendar/conflicts?start=&end= — overlapping events.
///
/// An event the reader declined is not a conflict: they are not going.
pub async fn get_conflicts(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Query(q): Query<ConflictsQuery>,
) -> Json<Vec<EventRow>> {
    let (Some(start_ts), Some(end_ts)) = (parse_iso(&q.start), parse_iso(&q.end)) else {
        return Json(vec![]);
    };
    let rows = rows_in_window(&user, start_ts, end_ts)
        .unwrap_or_default()
        .into_iter()
        .filter(|r| Some(r.uid.as_str()) != q.exclude_uid.as_deref())
        .filter(|r| r.partstat.as_deref() != Some("DECLINED"))
        // a day off or a holiday blocks no hour of it
        .filter(|r| !is_all_day(r))
        .collect();
    Json(rows)
}

#[derive(Debug, Deserialize)]
pub struct EventsQuery {
    /// ISO-8601 UTC start of the window.
    pub start: String,
    /// ISO-8601 UTC end of the window.
    pub end: String,
}

/// GET /api/calendar/events?start=&end= — every event overlapping the
/// window: the reader's own, invitations filed from mail, and each
/// subscribed feed's, declined ones included. 400 when either bound is
/// not an instant, 5xx when the store cannot be read — an empty week and
/// an unreadable one must not look the same.
pub async fn get_events(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Query(q): Query<EventsQuery>,
) -> Result<Json<Vec<EventRow>>, StatusCode> {
    let (Some(start_ts), Some(end_ts)) = (parse_iso(&q.start), parse_iso(&q.end)) else {
        return Err(StatusCode::BAD_REQUEST);
    };
    let mut rows = rows_in_window(&user, start_ts, end_ts)?;
    for row in &mut rows {
        let Some(mid) = row.message_id.as_deref() else {
            continue;
        };
        row.thread_id = state
            .core
            .find_by_message_id_for_user(&user, mid)
            .await
            .ok()
            .map(|m| m.thread_id.to_string());
    }
    Ok(Json(rows))
}

/// The user's events overlapping `[start_ts, end_ts)`, in start order.
///
/// One reader for the conflicts check and the calendar, so the two never
/// disagree about which events exist. A row without an end lasts an hour,
/// as it always has for conflicts.
fn rows_in_window(user: &str, start_ts: i64, end_ts: i64) -> Result<Vec<EventRow>, StatusCode> {
    use mailrs_core_sidestate::families::calendar_events::series;
    let user = user.to_string();
    let stored =
        with_kevy(move |c| mailrs_core_sidestate::families::calendar_events::list(c, &user))?;
    let (Some(from), Some(to)) = (
        chrono::DateTime::from_timestamp(start_ts, 0),
        chrono::DateTime::from_timestamp(end_ts, 0),
    ) else {
        return Err(StatusCode::BAD_REQUEST);
    };
    let mut rows = Vec::new();
    for ev in stored {
        if let Some(occurrences) = series::occurrences(&ev, from, to) {
            let row = EventRow::from(ev);
            rows.extend(occurrences.into_iter().map(|o| EventRow {
                dtstart: Some(o.start.to_column()),
                dtend: Some(o.end.to_column()),
                recurrence_id: Some(o.recurrence_id.to_column()),
                ..row.clone()
            }));
            continue;
        }
        let row = EventRow::from(ev);
        if overlaps(&row, start_ts, end_ts) {
            rows.push(row);
        }
    }
    rows.sort_by_key(|r| r.dtstart.as_deref().and_then(span::epoch_of));
    Ok(rows)
}

fn overlaps(row: &EventRow, start_ts: i64, end_ts: i64) -> bool {
    if is_all_day(row) {
        return all_day_overlaps(row, start_ts, end_ts);
    }
    let s_ts = row
        .dtstart
        .as_deref()
        .and_then(parse_iso)
        .unwrap_or(i64::MIN);
    let e_ts = row
        .dtend
        .as_deref()
        .and_then(parse_iso)
        .unwrap_or(s_ts + 3600);
    s_ts < end_ts && e_ts > start_ts
}

fn is_all_day(row: &EventRow) -> bool {
    row.dtstart.as_deref().and_then(span::date_of).is_some()
}

/// An all-day event's dates fall on different instants for readers in
/// different zones, and the window is the reader's days in UTC. Every
/// zone is within 14 hours of UTC, so widening by that much keeps every
/// event that touches the reader's days; the client places it exactly.
fn all_day_overlaps(row: &EventRow, start_ts: i64, end_ts: i64) -> bool {
    const WIDEST_OFFSET: i64 = 14 * 3600;
    let Some(s_ts) = row.dtstart.as_deref().and_then(span::epoch_of) else {
        return false;
    };
    let e_ts = row
        .dtend
        .as_deref()
        .and_then(span::epoch_of)
        .unwrap_or(s_ts + 86_400);
    s_ts - WIDEST_OFFSET < end_ts && e_ts + WIDEST_OFFSET > start_ts
}

impl From<mailrs_core_sidestate::families::calendar_events::StoredEvent> for EventRow {
    fn from(stored: mailrs_core_sidestate::families::calendar_events::StoredEvent) -> Self {
        Self {
            uid: stored.uid,
            summary: stored.summary,
            dtstart: stored.dtstart,
            dtend: stored.dtend,
            organizer: stored.organizer,
            status: stored.status,
            source: Some(stored.source).filter(|s| !s.is_empty()),
            partstat: stored.partstat,
            message_id: stored.message_id,
            thread_id: None,
            recurrence_id: None,
        }
    }
}
