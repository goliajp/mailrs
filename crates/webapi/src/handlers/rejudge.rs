//! Mail taken back into Review when it was judged again.
//!
//! - `GET /api/review/recalled` — the reader's own conversations taken back
//!   in the last seven days, newest first, for the line shown where each
//!   used to be
//! - `GET /api/admin/rejudge/batches` — every round that took mail back
//!   (super administrator)
//! - `POST /api/admin/rejudge/batches/{observation}/release` — put one round
//!   back whole (super administrator, recent password confirmation); each
//!   conversation is audited by the core, the round once here
//! - `POST /api/admin/rejudge/batches/{observation}/confirm` and
//!   `…/discard` — decide on a round that would have held more than one
//!   observation may on its own and so held nothing: hold what it recorded,
//!   or nothing (super administrator, recent password confirmation; the
//!   core and this tier each write an audit line)
//! - `GET /api/admin/fraud/rules/{rule}/triggers?days=30` — every delivery a
//!   rule under review fired on, one by one: recipients, sending domain,
//!   what the rule saw, when (super administrator: it names whose mail)

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;
use mailrs_core_sidestate::families::recall_notices;

/// GET /api/review/recalled
pub async fn recalled(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let now = chrono::Utc::now().timestamp();
    let items = with_kevy(move |c| recall_notices::recent(c, &user, now))?;
    Ok(Json(serde_json::json!({ "items": items })))
}

/// GET /api/admin/rejudge/batches
pub async fn batches(
    State(state): State<Arc<WebState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let found = state
        .core
        .rejudge_batches()
        .await
        .map_err(crate::handlers::admin::map_err)?;
    Ok(Json(serde_json::json!({ "items": found.items })))
}

/// GET /api/admin/fraud/rules/{rule}/triggers — up to 60 days back
/// (`days`, default 30), oldest first. 404 for a rule not under review,
/// so an empty list always means the rule did not fire.
pub async fn rule_triggers(
    Path(rule): Path<String>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use mailrs_core_sidestate::families::rule_triggers;
    if !rule_triggers::REVIEWED.contains(&rule.as_str()) {
        return Err(StatusCode::NOT_FOUND);
    }
    let days = q
        .get("days")
        .and_then(|d| d.parse::<i64>().ok())
        .unwrap_or(30)
        .clamp(1, rule_triggers::KEEP_DAYS);
    let now = chrono::Utc::now().timestamp();
    let items = with_kevy(move |c| rule_triggers::list(c, &rule, now, days))?;
    Ok(Json(serde_json::json!({ "days": days, "items": items })))
}

/// POST /api/admin/rejudge/batches/{observation}/release
pub async fn release(
    _: crate::session::confirm::Confirmed,
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(observation): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let done = state
        .core
        .release_rejudge_batch(&observation, &actor)
        .await
        .map_err(crate::handlers::admin::map_err)?;
    let detail = format!("released={}", done.released);
    crate::handlers::audit::record_required(
        &actor,
        "rejudge.batch_release",
        &observation,
        &detail,
    )?;
    Ok(Json(serde_json::json!({ "released": done.released })))
}

/// POST /api/admin/rejudge/batches/{observation}/confirm
pub async fn confirm(
    _: crate::session::confirm::Confirmed,
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(observation): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let done = state
        .core
        .confirm_rejudge_batch(&observation, &actor)
        .await
        .map_err(crate::handlers::admin::map_err)?;
    let detail = format!("held={}", done.held);
    crate::handlers::audit::record_required(
        &actor,
        "rejudge.batch_confirm",
        &observation,
        &detail,
    )?;
    Ok(Json(serde_json::json!({ "held": done.held })))
}

/// POST /api/admin/rejudge/batches/{observation}/discard
pub async fn discard(
    _: crate::session::confirm::Confirmed,
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(observation): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let done = state
        .core
        .discard_rejudge_batch(&observation, &actor)
        .await
        .map_err(crate::handlers::admin::map_err)?;
    let detail = format!("would_hold={}", done.would_hold);
    crate::handlers::audit::record_required(
        &actor,
        "rejudge.batch_discard",
        &observation,
        &detail,
    )?;
    Ok(Json(serde_json::json!({ "would_hold": done.would_hold })))
}
