//! Reporting a scam.
//!
//! - `POST /api/messages/{message_id}/report` — hold the reader's copy now;
//!   the other copies of the same template are taken back into Review, or
//!   wait for an administrator. A super administrator's report is a ground
//!   on its own.
//! - `GET /api/reports/{report_id}` — what came of the reader's own report
//! - `GET /api/admin/reports/pending` — reports whose copies wait
//! - `POST /api/admin/reports/{report_id}/confirm` — take them back (recent
//!   password confirmation)
//! - `POST /api/admin/reports/{report_id}/dismiss` — leave them

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use mailrs_core_api::method::report::{PendingReportsResponse, ReportOutcomeWire};

use crate::WebState;
use crate::handlers::admin::map_err;
use crate::handlers::conversations::AuthedUser;

/// POST /api/messages/{message_id}/report
pub async fn report(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(message_id): Path<String>,
) -> Result<Json<ReportOutcomeWire>, StatusCode> {
    let administrator = crate::handlers::kevy_util::require_super(&state, &user)
        .await
        .is_ok();
    let outcome = state
        .core
        .report_message(&user, &message_id, administrator)
        .await
        .map_err(map_err)?;
    Ok(Json(outcome))
}

/// GET /api/reports/{report_id}
pub async fn outcome(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(report_id): Path<String>,
) -> Result<Json<ReportOutcomeWire>, StatusCode> {
    let outcome = state
        .core
        .report_outcome(&user, &report_id)
        .await
        .map_err(map_err)?;
    Ok(Json(outcome))
}

/// GET /api/admin/reports/pending
pub async fn pending(
    State(state): State<Arc<WebState>>,
) -> Result<Json<PendingReportsResponse>, StatusCode> {
    let found = state.core.pending_reports().await.map_err(map_err)?;
    Ok(Json(found))
}

/// POST /api/admin/reports/{report_id}/confirm
pub async fn confirm(
    _: crate::session::confirm::Confirmed,
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(report_id): Path<String>,
) -> Result<Json<ReportOutcomeWire>, StatusCode> {
    let outcome = state
        .core
        .confirm_report(&report_id, &actor)
        .await
        .map_err(map_err)?;
    Ok(Json(outcome))
}

/// POST /api/admin/reports/{report_id}/dismiss
pub async fn dismiss(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(report_id): Path<String>,
) -> Result<Json<ReportOutcomeWire>, StatusCode> {
    let outcome = state
        .core
        .dismiss_report(&report_id, &actor)
        .await
        .map_err(map_err)?;
    Ok(Json(outcome))
}
