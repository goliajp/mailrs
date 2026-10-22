//! Domain contract routes backed by fastcore's embedded kevy. These
//! mirror the pg-core admin handlers so both cores serve the identical
//! `/v1/admin/domains` surface.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use mailrs_core_api::method::admin::{AddDomainRequest, DomainListResponse, DomainWire};

use crate::FastcoreState;

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub async fn list_domains(
    State(state): State<Arc<FastcoreState>>,
) -> Result<Json<DomainListResponse>, StatusCode> {
    let rows = state
        .mailbox
        .list_domains()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let items = rows
        .into_iter()
        .map(|(name, created_at)| DomainWire { name, created_at })
        .collect();
    Ok(Json(DomainListResponse { items }))
}

pub async fn add_domain(
    State(state): State<Arc<FastcoreState>>,
    Json(req): Json<AddDomainRequest>,
) -> Result<StatusCode, StatusCode> {
    state
        .mailbox
        .upsert_domain(&req.name, now_secs())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_domain(
    State(state): State<Arc<FastcoreState>>,
    Path(name): Path<String>,
) -> StatusCode {
    match state.mailbox.delete_domain(&name) {
        Ok(true) => StatusCode::NO_CONTENT,
        Ok(false) => StatusCode::NOT_FOUND,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
