//! The ops daemon's administrator routes, for the admin page. Super
//! administrators only (`route_permissions`); setting or ending a silence
//! and changing maintenance also need a recent confirmation. The calls to
//! the daemon are in [`link`].

pub mod link;

use axum::Json;
use axum::extract::{Extension, Path, Query};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};

pub use link::{MaintenanceAction, MaintenanceRequest, Refusal, SilenceRequest, silent_after_secs};

use crate::handlers::conversations::AuthedUser;

type Answer = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn answer(r: Result<Value, Refusal>) -> Answer {
    r.map(Json)
        .map_err(|e| (e.status, Json(json!({ "error": e.message }))))
}

/// GET /api/admin/ops/state — the daemon's state, or why it is not there.
pub async fn state(Extension(AuthedUser(actor)): Extension<AuthedUser>) -> Json<Value> {
    Json(link::read_state(&actor).await)
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    since: Option<i64>,
    limit: Option<u32>,
}

/// GET /api/admin/ops/history — alert transitions, notifications and
/// silence / maintenance events since `since` (the last day by default).
pub async fn history(
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Query(q): Query<HistoryQuery>,
) -> Answer {
    answer(link::read_history(&actor, q.since, q.limit).await)
}

/// POST /api/admin/ops/silences — silence one rule (or one object of it)
/// for `for_secs` from now, for a reason.
pub async fn set_silence(
    _: crate::session::confirm::Confirmed,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Json(req): Json<SilenceRequest>,
) -> Answer {
    answer(link::silence(&actor, &req).await)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndSilenceRequest {
    pub reason: String,
}

/// POST /api/admin/ops/silences/{id}/end — end a silence before its time.
pub async fn end_silence(
    _: crate::session::confirm::Confirmed,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(id): Path<String>,
    Json(req): Json<EndSilenceRequest>,
) -> Answer {
    answer(link::unsilence(&actor, &id, &req.reason).await)
}

/// POST /api/admin/ops/maintenance — start, extend or end maintenance.
pub async fn maintenance(
    _: crate::session::confirm::Confirmed,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Json(req): Json<MaintenanceRequest>,
) -> Answer {
    answer(link::change_maintenance(&actor, &req).await)
}
