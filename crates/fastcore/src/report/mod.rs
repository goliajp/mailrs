//! Reporting a scam: one person's report holds their copy at once, and the
//! other copies of the same template are taken back into Review, or wait
//! for an administrator (`mailrs_report::decide` says when). The flow is
//! `mailrs_intake::scam_report`, shared with the SQL core.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_core_api::method::report::{
    DecideReportRequest, PendingReportWire, PendingReportsResponse, ReportRequest,
};
use mailrs_core_sidestate::families::reports;
use mailrs_intake::scam_report;

use crate::FastcoreState;

pub(crate) mod fingerprint;

use fingerprint::Reports;

fn pending_wire(p: scam_report::Pending) -> PendingReportWire {
    reports::pending_to_wire(&p.report, &p.outcome, p.subject, p.from, p.samples)
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

/// `POST /v1/users/{user}/messages/{message_id}/report`
pub(crate) async fn report(
    State(state): State<Arc<FastcoreState>>,
    Path((user, mid)): Path<(String, String)>,
    Json(req): Json<ReportRequest>,
) -> Response {
    let found = tokio::task::spawn_blocking(move || {
        scam_report::report(&Reports(&state), &user, &mid, req.administrator)
    })
    .await;
    answer(found, "report")
}

/// `GET /v1/users/{user}/reports/{report_id}` — the reporter's own only.
pub(crate) async fn outcome(
    State(state): State<Arc<FastcoreState>>,
    Path((user, id)): Path<(String, String)>,
) -> Response {
    let found =
        tokio::task::spawn_blocking(move || scam_report::outcome_for(&Reports(&state), &user, &id))
            .await;
    answer(found, "report outcome")
}

/// `GET /v1/admin/reports/pending`
pub(crate) async fn pending(State(state): State<Arc<FastcoreState>>) -> Response {
    match tokio::task::spawn_blocking(move || scam_report::pending(&Reports(&state))).await {
        Ok(Ok(items)) => Json(PendingReportsResponse {
            items: items.into_iter().map(pending_wire).collect(),
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

/// `POST /v1/admin/reports/{report_id}/confirm`
pub(crate) async fn confirm(
    State(state): State<Arc<FastcoreState>>,
    Path(id): Path<String>,
    Json(req): Json<DecideReportRequest>,
) -> Response {
    let found = tokio::task::spawn_blocking(move || {
        scam_report::confirm(&Reports(&state), &id, &req.actor)
    })
    .await;
    answer(found, "report confirm")
}

/// `POST /v1/admin/reports/{report_id}/dismiss`
pub(crate) async fn dismiss(
    State(state): State<Arc<FastcoreState>>,
    Path(id): Path<String>,
    Json(req): Json<DecideReportRequest>,
) -> Response {
    let found = tokio::task::spawn_blocking(move || {
        scam_report::dismiss(&Reports(&state), &id, &req.actor)
    })
    .await;
    answer(found, "report dismiss")
}

#[derive(serde::Deserialize)]
pub(crate) struct FootprintQuery {
    limit: Option<usize>,
}

/// `GET /v1/admin/maintenance:template-footprint?limit=` — read-only: how
/// many fingerprints and findable index rows are stored, the bytes their
/// fields hold, and that per ten thousand messages. For sizing on a copy:
/// it writes nothing. Bytes are the stored keys and fields, not the
/// store's own overhead; compare the store's memory before and after for
/// that.
pub(crate) async fn footprint(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<FootprintQuery>,
) -> Response {
    let limit = q.limit.unwrap_or(100_000).clamp(1, 1_000_000);
    match tokio::task::spawn_blocking(move || state.mailbox.template_footprint(limit)).await {
        Ok(Ok(f)) => {
            let per = |bytes: u64| match f.fingerprints {
                0 => serde_json::Value::Null,
                n => serde_json::json!(bytes * 10_000 / n),
            };
            Json(serde_json::json!({
                "fingerprints": f.fingerprints,
                "fingerprint_bytes": f.fingerprint_bytes,
                "segment_rows": f.segment_rows,
                "segment_bytes": f.segment_bytes,
                "index_entries": f.segment_rows * 5,
                "capped": f.capped,
                "fingerprint_bytes_per_10k": per(f.fingerprint_bytes),
                "segment_bytes_per_10k": per(f.segment_bytes),
            }))
            .into_response()
        }
        Ok(Err(e)) => {
            tracing::error!(error = %e, "template footprint failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
