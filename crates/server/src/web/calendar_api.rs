//! Calendar-side HTTP endpoints for the web client (MRS-5 + MRS-6).
//!
//! - `GET /api/calendar/conflicts` — given a [start, end) window, return
//!   active calendar events that overlap it. Used by the invite-card UI to
//!   show conflict hints next to Accept/Tentative/Decline buttons.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{AuthUser, WebState};

#[derive(Deserialize)]
pub(super) struct ConflictsQuery {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    #[serde(default)]
    pub exclude_uid: Option<String>,
}

#[derive(Serialize)]
pub(super) struct ConflictRow {
    pub uid: String,
    pub summary: String,
    pub dtstart: Option<DateTime<Utc>>,
    pub dtend: Option<DateTime<Utc>>,
    pub organizer: Option<String>,
    pub status: Option<String>,
}

pub(super) async fn get_conflicts(
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Query(q): Query<ConflictsQuery>,
) -> impl IntoResponse {
    let Some(ref pool) = state.pg_pool else {
        return Json(Vec::<ConflictRow>::new());
    };

    // Find user's default calendar id (calendars table is keyed by
    // account_address; users typically have a single "Default" one).
    let cal_id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM calendars
         WHERE account_address = $1
         ORDER BY id ASC LIMIT 1",
    )
    .bind(&user)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    let Some(cal_id) = cal_id else {
        return Json(Vec::<ConflictRow>::new());
    };

    let conflicts = crate::calendar::event::find_conflicts(
        pool,
        cal_id,
        q.start,
        q.end,
        q.exclude_uid.as_deref(),
    )
    .await
    .unwrap_or_default();

    let rows: Vec<ConflictRow> = conflicts
        .into_iter()
        .map(|c| ConflictRow {
            uid: c.uid,
            summary: c.summary,
            dtstart: c.dtstart,
            dtend: c.dtend,
            organizer: c.organizer,
            status: c.status,
        })
        .collect();

    Json(rows)
}

// ── MRS-10: external WebDAV ICS feed subscription endpoints ──────────

#[derive(Serialize)]
pub(super) struct FeedRow {
    pub id: i64,
    pub url: String,
    pub name: String,
    pub refresh_interval_secs: i32,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub enabled: bool,
}

#[derive(Deserialize)]
pub(super) struct CreateFeedReq {
    pub url: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub basic_auth_user: Option<String>,
    #[serde(default)]
    pub basic_auth_pass: Option<String>,
    /// Polling cadence in seconds; default 15min if missing.
    #[serde(default)]
    pub refresh_interval_secs: Option<i32>,
}

#[derive(Serialize)]
pub(super) struct CreateFeedResp {
    pub id: i64,
}

pub(super) async fn list_feeds(
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref pool) = state.pg_pool else {
        return Json(Vec::<FeedRow>::new());
    };
    let feeds = crate::calendar::feed::list_for_account(pool, &user)
        .await
        .unwrap_or_default();
    let rows: Vec<FeedRow> = feeds
        .into_iter()
        .map(|f| FeedRow {
            id: f.id,
            url: f.url,
            name: f.name,
            refresh_interval_secs: f.refresh_interval_secs,
            last_synced_at: f.last_synced_at,
            last_error: f.last_error,
            enabled: f.enabled,
        })
        .collect();
    Json(rows)
}

pub(super) async fn create_feed(
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<CreateFeedReq>,
) -> impl IntoResponse {
    let Some(ref pool) = state.pg_pool else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"success": false, "message": "postgres unavailable"})),
        )
            .into_response();
    };

    // Minimum sanity: must be http(s).
    let url_trimmed = req.url.trim();
    if !(url_trimmed.starts_with("http://") || url_trimmed.starts_with("https://")) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"success": false, "message": "url must be http(s)"})),
        )
            .into_response();
    }
    let interval = req.refresh_interval_secs.unwrap_or(900).max(60);

    match crate::calendar::feed::create(
        pool,
        crate::calendar::feed::CreateFeed {
            account_address: &user,
            url: url_trimmed,
            name: &req.name,
            basic_auth_user: req.basic_auth_user.as_deref(),
            basic_auth_pass: req.basic_auth_pass.as_deref(),
            refresh_interval_secs: interval,
        },
    )
    .await
    {
        Ok(id) => Json(CreateFeedResp { id }).into_response(),
        Err(e) => {
            tracing::warn!("create feed failed for {user}: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"success": false, "message": format!("{e}")})),
            )
                .into_response()
        }
    }
}

pub(super) async fn delete_feed(
    Path(feed_id): Path<i64>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref pool) = state.pg_pool else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    match crate::calendar::feed::delete(pool, &user, feed_id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::warn!("delete feed failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
