//! `/api/calendar/*` endpoints — feeds CRUD + conflicts.
//!
//! Storage on network kevy:
//!
//!   calendar_feeds:<user>          hash { feed_id -> JSON CalendarFeed }
//!   calendar_events:<user>         zset  score = dtstart_epoch, member = event_uid
//!   calendar_event:<user>:<uid>    hash  { summary, dtstart, dtend, organizer, status, source }
//!
//! Full recurrence expansion (RRULE / EXDATE / RECURRENCE-ID) mirrors
//! the monolith's `crates/server/src/web/calendar_api.rs`. This port
//! ships the same wire shapes so the UI's invite card + weekly view
//! render identically.

use axum::extract::{Extension, Path, Query};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

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
pub async fn get_conflicts(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Query(q): Query<ConflictsQuery>,
) -> Json<Vec<EventRow>> {
    let Some(start_ts) = parse_iso(&q.start) else {
        return Json(vec![]);
    };
    let Some(end_ts) = parse_iso(&q.end) else {
        return Json(vec![]);
    };
    let idx_key = format!("calendar_events:{user}");
    let user_c = user.clone();
    let members = with_kevy(move |c| c.zrange(idx_key.as_bytes(), 0, -1)).unwrap_or_default();
    let mut out = Vec::new();
    for m in members {
        let Some(uid) = String::from_utf8(m).ok() else {
            continue;
        };
        if Some(uid.as_str()) == q.exclude_uid.as_deref() {
            continue;
        }
        let key = format!("calendar_event:{user_c}:{uid}");
        let flat = with_kevy(move |c| c.hgetall(key.as_bytes())).unwrap_or_default();
        let mut row = EventRow {
            uid: uid.clone(),
            ..Default::default()
        };
        let mut i = 0;
        while i + 1 < flat.len() {
            let k = String::from_utf8_lossy(&flat[i]);
            let v = String::from_utf8_lossy(&flat[i + 1]).to_string();
            match k.as_ref() {
                "summary" => row.summary = v,
                "dtstart" => row.dtstart = Some(v),
                "dtend" => row.dtend = Some(v),
                "organizer" => row.organizer = Some(v),
                "status" => row.status = Some(v),
                "source" => row.source = Some(v),
                _ => {}
            }
            i += 2;
        }
        // Window overlap check.
        let s_ts = row.dtstart.as_deref().and_then(parse_iso).unwrap_or(i64::MIN);
        let e_ts = row.dtend.as_deref().and_then(parse_iso).unwrap_or(s_ts + 3600);
        if s_ts < end_ts && e_ts > start_ts {
            out.push(row);
        }
    }
    Json(out)
}


// ── Feeds (subscriptions to external ICS URLs) ─────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FeedWire {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub sync_interval_secs: i64,
    pub created_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateFeedRequest {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default = "default_sync_interval")]
    pub sync_interval_secs: i64,
}

fn default_sync_interval() -> i64 {
    3600
}

fn random_id() -> String {
    let mut b = [0u8; 8];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// GET /api/calendar/feeds — user's subscribed feeds.
pub async fn list_feeds(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Json<serde_json::Value> {
    let key = format!("calendar_feeds:{user}");
    let flat = with_kevy(move |c| c.hgetall(key.as_bytes())).unwrap_or_default();
    let mut items = Vec::new();
    let mut i = 0;
    while i + 1 < flat.len() {
        if let Ok(f) = serde_json::from_slice::<FeedWire>(&flat[i + 1]) {
            items.push(f);
        }
        i += 2;
    }
    Json(serde_json::json!({ "items": items }))
}

/// POST /api/calendar/feeds — subscribe.
pub async fn create_feed(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<CreateFeedRequest>,
) -> Result<Json<FeedWire>, StatusCode> {
    let feed = FeedWire {
        id: random_id(),
        name: req.name,
        url: req.url,
        color: req.color,
        sync_interval_secs: req.sync_interval_secs,
        created_at: now_secs(),
    };
    let key = format!("calendar_feeds:{user}");
    let id_c = feed.id.clone();
    let payload = serde_json::to_vec(&feed).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    with_kevy(move |c| {
        c.hset(key.as_bytes(), &[(id_c.as_bytes(), payload.as_slice())])?;
        Ok(())
    })?;
    Ok(Json(feed))
}

/// DELETE /api/calendar/feeds/{feed_id} — unsubscribe.
pub async fn delete_feed(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(feed_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("calendar_feeds:{user}");
    with_kevy(move |c| {
        c.hdel(key.as_bytes(), &[feed_id.as_bytes()])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}
