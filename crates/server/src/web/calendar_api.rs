//! Calendar-side HTTP endpoints for the web client (MRS-5 + MRS-6).
//!
//! - `GET /api/calendar/conflicts` — given a [start, end) window, return
//!   active calendar events that overlap it. Used by the invite-card UI to
//!   show conflict hints next to Accept/Tentative/Decline buttons.

use std::sync::Arc;

use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
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
