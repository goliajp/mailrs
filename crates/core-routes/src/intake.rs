//! Filing a message a user brought or sent, and reporting a scam: the
//! routes over the flows in `mailrs_intake`, which run on a blocking
//! thread against the store's intake core.

use std::io;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_core_api::method::import::ImportMessageRequest;
use mailrs_core_api::method::report::{DecideReportRequest, PendingReportsResponse, ReportRequest};
use mailrs_core_api::method::sent::{FileSentRequest, FileSentResponse};
use mailrs_core_sidestate::families::reports;
use mailrs_intake::scam_report;
use mailrs_intake::sent::Filed;
use tokio::task::JoinError;

use crate::store::IntakeStore;

fn filed_answer(
    filed: Result<io::Result<Filed>, JoinError>,
    user: &str,
    what: &str,
) -> Result<Json<FileSentResponse>, StatusCode> {
    match filed {
        Ok(Ok(f)) => Ok(Json(FileSentResponse {
            thread_id: f.thread_id,
            message_id: f.message_id,
            blob_ref: f.blob_ref,
        })),
        Ok(Err(e)) => {
            tracing::error!(error = %e, %user, "{what} failed");
            match e.kind() {
                io::ErrorKind::InvalidInput => Err(StatusCode::BAD_REQUEST),
                io::ErrorKind::QuotaExceeded => Err(StatusCode::INSUFFICIENT_STORAGE),
                _ => Err(StatusCode::INTERNAL_SERVER_ERROR),
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "{what} panicked");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// POST /v1/users/{user}/sent — the sender's own copy of a message the web
/// tier queued, or of one brought over from another mailbox's Sent.
pub async fn file_sent<S: IntakeStore>(
    State(state): State<Arc<S>>,
    Path(user): Path<String>,
    Json(req): Json<FileSentRequest>,
) -> Result<Json<FileSentResponse>, StatusCode> {
    let body = req.message().map_err(|_| StatusCode::BAD_REQUEST)?;
    let core = state.intake_core();
    let who = user.clone();
    let filed = tokio::task::spawn_blocking(move || match req.imported {
        true => mailrs_intake::sent::file_imported(&core, &user, &body, &req.recipients),
        false => mailrs_intake::sent::file(&core, &user, &body, &req.recipients),
    })
    .await;
    filed_answer(filed, &who, "filing the sent copy")
}

/// POST /v1/users/{user}/import — a message brought over from another
/// mailbox.
pub async fn import_message<S: IntakeStore>(
    State(state): State<Arc<S>>,
    Path(user): Path<String>,
    Json(req): Json<ImportMessageRequest>,
) -> Result<Json<FileSentResponse>, StatusCode> {
    let body = req.message().map_err(|_| StatusCode::BAD_REQUEST)?;
    let verdict = req.fraud_verdict;
    let core = state.intake_core();
    let who = user.clone();
    let filed = tokio::task::spawn_blocking(move || {
        mailrs_intake::import::file(&core, &user, &body, verdict.as_deref())
    })
    .await;
    filed_answer(filed, &who, "importing a message")
}

type Found = Result<Option<(reports::Report, reports::Outcome)>, String>;

fn report_answer(found: Result<Found, JoinError>, what: &str) -> Response {
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

/// POST /v1/users/{user}/messages/{message_id}/report — one person's
/// report holds their copy at once; the other copies of the same template
/// are taken back into Review or wait for an administrator.
pub async fn report<S: IntakeStore>(
    State(state): State<Arc<S>>,
    Path((user, mid)): Path<(String, String)>,
    Json(req): Json<ReportRequest>,
) -> Response {
    let core = state.intake_core();
    let found = tokio::task::spawn_blocking(move || {
        scam_report::report(&core, &user, &mid, req.administrator)
    })
    .await;
    report_answer(found, "report")
}

/// GET /v1/users/{user}/reports/{report_id} — the reporter's own only.
pub async fn outcome<S: IntakeStore>(
    State(state): State<Arc<S>>,
    Path((user, id)): Path<(String, String)>,
) -> Response {
    let core = state.intake_core();
    let found =
        tokio::task::spawn_blocking(move || scam_report::outcome_for(&core, &user, &id)).await;
    report_answer(found, "report outcome")
}

/// GET /v1/admin/reports/pending
pub async fn pending<S: IntakeStore>(State(state): State<Arc<S>>) -> Response {
    let core = state.intake_core();
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
pub async fn confirm<S: IntakeStore>(
    State(state): State<Arc<S>>,
    Path(id): Path<String>,
    Json(req): Json<DecideReportRequest>,
) -> Response {
    let core = state.intake_core();
    let found =
        tokio::task::spawn_blocking(move || scam_report::confirm(&core, &id, &req.actor)).await;
    report_answer(found, "report confirm")
}

/// POST /v1/admin/reports/{report_id}/dismiss
pub async fn dismiss<S: IntakeStore>(
    State(state): State<Arc<S>>,
    Path(id): Path<String>,
    Json(req): Json<DecideReportRequest>,
) -> Response {
    let core = state.intake_core();
    let found =
        tokio::task::spawn_blocking(move || scam_report::dismiss(&core, &id, &req.actor)).await;
    report_answer(found, "report dismiss")
}
