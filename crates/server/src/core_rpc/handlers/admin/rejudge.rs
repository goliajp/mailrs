//! Rounds of re-judging. This core judges mail when it arrives and when the
//! rules change, never again for one observation, so it took nothing back:
//! the list is empty and any batch is missing.

use axum::Json;
use axum::extract::Path;
use axum::http::StatusCode;

use mailrs_core_api::method::admin as wire;

/// GET /v1/admin/rejudge-batches
pub async fn rejudge_batches() -> Json<wire::RejudgeBatchesResponse> {
    Json(wire::RejudgeBatchesResponse { items: Vec::new() })
}

/// POST /v1/admin/rejudge-batches/{observation}/release
pub async fn release_rejudge_batch(
    Path(_observation): Path<String>,
    Json(_req): Json<wire::ReleaseRejudgeBatchRequest>,
) -> StatusCode {
    StatusCode::NOT_FOUND
}
