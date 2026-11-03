//! The administrator's view of what re-judging took back, and putting a
//! round back whole.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_core_api::method::admin::{
    RejudgeBatchWire, RejudgeBatchesResponse, ReleaseRejudgeBatchRequest,
    ReleaseRejudgeBatchResponse,
};

use super::recall;
use crate::FastcoreState;

/// `GET /v1/admin/rejudge-batches`
pub(crate) async fn batches(State(state): State<Arc<FastcoreState>>) -> Response {
    let found = tokio::task::spawn_blocking(move || recall::batches(&state)).await;
    match found {
        Ok(items) => Json(RejudgeBatchesResponse {
            items: items
                .into_iter()
                .map(|b| RejudgeBatchWire {
                    observation: b.observation,
                    at: b.at,
                    conversations: b.conversations,
                    released_at: b.released_at,
                })
                .collect(),
        })
        .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "rejudge batches: listing panicked");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `POST /v1/admin/rejudge-batches/{observation}/release`
pub(crate) async fn release(
    State(state): State<Arc<FastcoreState>>,
    Path(observation): Path<String>,
    Json(req): Json<ReleaseRejudgeBatchRequest>,
) -> Response {
    let done = tokio::task::spawn_blocking(move || {
        recall::release_batch(&state, &observation, &req.actor)
    })
    .await;
    match done {
        Ok(Some(released)) => Json(ReleaseRejudgeBatchResponse { released }).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::error!(error = %e, "rejudge batch release panicked");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
