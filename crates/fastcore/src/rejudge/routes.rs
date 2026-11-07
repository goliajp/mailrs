//! The administrator's view of what re-judging took back, putting a round
//! back whole, and deciding on a round that waits.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_core_api::method::admin::{
    ConfirmRejudgeBatchResponse, DecideRejudgeBatchRequest, DiscardRejudgeBatchResponse,
    RejudgeBatchWire, RejudgeBatchesResponse, ReleaseRejudgeBatchRequest,
    ReleaseRejudgeBatchResponse,
};

use super::recall::{self, Refused};
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
                    pending: b.pending,
                    would_hold: b.would_hold,
                    rules: b.rules,
                    discarded_at: b.discarded_at,
                    confirmed_at: b.confirmed_at,
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

/// Run a decision on one batch off the runtime and answer with `ok` of its
/// count, 404 for a missing batch, 409 for one in the wrong state.
async fn decided<T: serde::Serialize>(
    what: &'static str,
    run: impl FnOnce() -> Result<u64, Refused> + Send + 'static,
    ok: impl FnOnce(u64) -> T,
) -> Response {
    match tokio::task::spawn_blocking(run).await {
        Ok(Ok(n)) => Json(ok(n)).into_response(),
        Ok(Err(Refused::Missing)) => StatusCode::NOT_FOUND.into_response(),
        Ok(Err(Refused::Conflict)) => StatusCode::CONFLICT.into_response(),
        Err(e) => {
            tracing::error!(error = %e, what, "rejudge batch decision panicked");
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
    decided(
        "release",
        move || recall::release_batch(&state, &observation, &req.actor),
        |released| ReleaseRejudgeBatchResponse { released },
    )
    .await
}

/// `POST /v1/admin/rejudge-batches/{observation}/confirm`
pub(crate) async fn confirm(
    State(state): State<Arc<FastcoreState>>,
    Path(observation): Path<String>,
    Json(req): Json<DecideRejudgeBatchRequest>,
) -> Response {
    decided(
        "confirm",
        move || recall::confirm_batch(&state, &observation, &req.actor),
        |held| ConfirmRejudgeBatchResponse { held },
    )
    .await
}

/// `POST /v1/admin/rejudge-batches/{observation}/discard`
pub(crate) async fn discard(
    State(state): State<Arc<FastcoreState>>,
    Path(observation): Path<String>,
    Json(req): Json<DecideRejudgeBatchRequest>,
) -> Response {
    decided(
        "discard",
        move || recall::discard_batch(&state, &observation, &req.actor),
        |would_hold| DiscardRejudgeBatchResponse { would_hold },
    )
    .await
}
