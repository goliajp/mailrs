//! Reporting a scam, on this core: the routes over the flow both cores
//! share (`mailrs_intake::scam_report`).

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_core_api::method::report::{DecideReportRequest, PendingReportsResponse, ReportRequest};
use mailrs_core_sidestate::families::reports;
use mailrs_intake::scam_report;

use crate::core_rpc::CoreRpcState;
use crate::intake::{PgCore, PgDirectory};

/// Built on a runtime thread: the directory keeps its handle.
fn core(state: &CoreRpcState) -> PgCore {
    PgCore {
        dir: PgDirectory {
            authserv_id: state.authserv_id.clone(),
            ..PgDirectory::new(
                state.domain.clone(),
                state.maildir_root.clone(),
                state.net_url.clone(),
            )
        },
        mailbox: state.mailbox.clone(),
        event_bus: state.event_bus.clone(),
    }
}

type Found = Result<Option<(reports::Report, reports::Outcome)>, String>;

fn answer(found: Result<Found, tokio::task::JoinError>, what: &str) -> Response {
    match found {
        Ok(Ok(Some((r, o)))) => Json(reports::to_wire(&r, &o)).into_response(),
        Ok(Ok(None)) => StatusCode::NOT_FOUND.into_response(),
        Ok(Err(e)) => {
            tracing::error!(error = %e, "{what} failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "{what} panicked");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// POST /v1/users/{user}/messages/{message_id}/report
pub async fn report(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, mid)): Path<(String, String)>,
    Json(req): Json<ReportRequest>,
) -> Response {
    let core = core(&state);
    let found = tokio::task::spawn_blocking(move || {
        scam_report::report(&core, &user, &mid, req.administrator)
    })
    .await;
    answer(found, "report")
}

/// GET /v1/users/{user}/reports/{report_id}
pub async fn outcome(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, id)): Path<(String, String)>,
) -> Response {
    let core = core(&state);
    let found =
        tokio::task::spawn_blocking(move || scam_report::outcome_for(&core, &user, &id)).await;
    answer(found, "report outcome")
}

/// GET /v1/admin/reports/pending
pub async fn pending(State(state): State<Arc<CoreRpcState>>) -> Response {
    let core = core(&state);
    match tokio::task::spawn_blocking(move || scam_report::pending(&core)).await {
        Ok(Ok(items)) => Json(PendingReportsResponse {
            items: items
                .into_iter()
                .map(|p| {
                    reports::pending_to_wire(&p.report, &p.outcome, p.subject, p.from, p.samples)
                })
                .collect(),
        })
        .into_response(),
        Ok(Err(e)) => {
            tracing::error!(error = %e, "pending reports failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "pending reports panicked");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// POST /v1/admin/reports/{report_id}/confirm
pub async fn confirm(
    State(state): State<Arc<CoreRpcState>>,
    Path(id): Path<String>,
    Json(req): Json<DecideReportRequest>,
) -> Response {
    let core = core(&state);
    let found =
        tokio::task::spawn_blocking(move || scam_report::confirm(&core, &id, &req.actor)).await;
    answer(found, "report confirm")
}

/// POST /v1/admin/reports/{report_id}/dismiss
pub async fn dismiss(
    State(state): State<Arc<CoreRpcState>>,
    Path(id): Path<String>,
    Json(req): Json<DecideReportRequest>,
) -> Response {
    let core = core(&state);
    let found =
        tokio::task::spawn_blocking(move || scam_report::dismiss(&core, &id, &req.actor)).await;
    answer(found, "report dismiss")
}
