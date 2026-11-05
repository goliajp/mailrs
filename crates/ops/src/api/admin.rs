//! The administrator routes: state and history out, silences and
//! maintenance in. Every change names who made it and why, and has an end.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;
use sha2::Digest;

use super::Caller;
use crate::app::App;
use crate::store::Silence;

/// Maintenance opened without an end runs this long.
pub const DEFAULT_MAINTENANCE_SECS: i64 = 2 * 3600;

type Answer = Result<Json<serde_json::Value>, (StatusCode, String)>;

fn bad(why: impl Into<String>) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, why.into())
}

fn internal(why: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, why.to_string())
}

/// Who to record: the administrator webapi names, or `--by` on the socket.
fn who(caller: &Caller, by: Option<&str>) -> Result<String, (StatusCode, String)> {
    let named = |s: &str| !s.trim().is_empty();
    match (caller, by) {
        (Caller::Webapi { actor: Some(a) }, _) => Ok(format!("webapi:{a}")),
        (Caller::Webapi { actor: None }, _) => Err(bad("x-mailrs-ops-actor names nobody")),
        (Caller::Socket, Some(b)) if named(b) => Ok(format!("cli:{}", b.trim())),
        (Caller::Socket, _) => Err(bad("say who you are with --by")),
    }
}

/// The end of a silence or maintenance stretch: given, or now plus a span.
fn end(now: i64, until: Option<i64>, for_secs: Option<i64>) -> Option<i64> {
    until.or(for_secs.map(|s| now + s)).filter(|t| *t > now)
}

pub(super) async fn state(State(app): State<Arc<App>>) -> Answer {
    app.state().await.map(Json).map_err(internal)
}

#[derive(Deserialize)]
pub(super) struct HistoryQuery {
    since: Option<i64>,
    limit: Option<u32>,
}

pub(super) async fn history(State(app): State<Arc<App>>, Query(q): Query<HistoryQuery>) -> Answer {
    let since = q.since.unwrap_or(app.clock.now() - 86_400);
    app.history(since, q.limit.unwrap_or(200).min(1000))
        .map(Json)
        .map_err(internal)
}

#[derive(Deserialize)]
pub(super) struct SilenceRequest {
    rule: String,
    object: Option<String>,
    until: Option<i64>,
    for_secs: Option<i64>,
    reason: String,
    by: Option<String>,
}

pub(super) async fn set_silence(
    State(app): State<Arc<App>>,
    Extension(caller): Extension<Caller>,
    Json(req): Json<SilenceRequest>,
) -> Answer {
    let by = who(&caller, req.by.as_deref())?;
    let now = app.clock.now();
    if !crate::rules::all().iter().any(|r| r.name == req.rule) {
        return Err(bad(format!("no rule named {:?}", req.rule)));
    }
    if req.reason.trim().is_empty() {
        return Err(bad("a silence needs a reason"));
    }
    let until =
        end(now, req.until, req.for_secs).ok_or(bad("a silence needs an end in the future"))?;
    let seed = format!("{now}-{}-{:?}-{by}", req.rule, req.object);
    let silence = Silence {
        id: hex::encode(&sha2::Sha256::digest(seed.as_bytes())[..6]),
        rule: req.rule,
        object: req.object.filter(|o| !o.trim().is_empty()),
        until,
        reason: req.reason,
        by,
        set_at: now,
    };
    app.store.set_silence(&silence).map_err(internal)?;
    Ok(Json(serde_json::to_value(&silence).map_err(internal)?))
}

#[derive(Deserialize)]
pub(super) struct EndRequest {
    by: Option<String>,
    reason: Option<String>,
}

pub(super) async fn end_silence(
    State(app): State<Arc<App>>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<String>,
    Query(req): Query<EndRequest>,
) -> Answer {
    let by = who(&caller, req.by.as_deref())?;
    let reason = req.reason.unwrap_or_else(|| "removed".into());
    match app
        .store
        .end_silence(&id, "removed", &by, &reason, app.clock.now())
    {
        Ok(true) => Ok(Json(serde_json::json!({ "removed": id }))),
        Ok(false) => Err((StatusCode::NOT_FOUND, format!("no silence {id} in force"))),
        Err(e) => Err(internal(e)),
    }
}

#[derive(Deserialize)]
pub(super) struct MaintenanceRequest {
    action: String,
    until: Option<i64>,
    for_secs: Option<i64>,
    reason: String,
    by: Option<String>,
}

pub(super) async fn maintenance(
    State(app): State<Arc<App>>,
    Extension(caller): Extension<Caller>,
    Json(req): Json<MaintenanceRequest>,
) -> Answer {
    let by = who(&caller, req.by.as_deref())?;
    if req.reason.trim().is_empty() {
        return Err(bad("maintenance needs a reason"));
    }
    let now = app.clock.now();
    let current = app.store.maintenance(now).map_err(internal)?;
    let conflict = |why: &str| (StatusCode::CONFLICT, why.to_string());
    match (req.action.as_str(), &current) {
        ("start", Some(_)) => return Err(conflict("maintenance is already on; extend it")),
        ("start", None) => {
            let until = match (req.until, req.for_secs) {
                (None, None) => now + DEFAULT_MAINTENANCE_SECS,
                (u, f) => end(now, u, f).ok_or(bad("maintenance needs an end in the future"))?,
            };
            app.store
                .maintenance_event("start", until, &req.reason, &by, now)
                .map_err(internal)?;
        }
        ("extend", None) | ("end", None) => return Err(conflict("maintenance is not on")),
        ("extend", Some(_)) => {
            let until = end(now, req.until, req.for_secs)
                .ok_or(bad("extending needs an end in the future"))?;
            app.store
                .maintenance_event("extend", until, &req.reason, &by, now)
                .map_err(internal)?;
        }
        ("end", Some(m)) => {
            app.store
                .maintenance_event("end", m.until, &req.reason, &by, now)
                .map_err(internal)?;
            app.dispatcher.lock().await.maintenance_ended(now).await;
        }
        (other, _) => {
            return Err(bad(format!(
                "unknown action {other:?}: start, extend or end"
            )));
        }
    }
    let now_state = app.store.maintenance(now).map_err(internal)?;
    Ok(Json(serde_json::json!({ "maintenance": now_state })))
}

pub(super) async fn test_channels(State(app): State<Arc<App>>) -> Answer {
    let now = app.clock.now();
    let results = app.dispatcher.lock().await.test(now).await;
    Ok(Json(serde_json::json!({ "results": results })))
}
