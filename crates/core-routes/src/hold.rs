//! Letting go of a held conversation, and what was let go before.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_core_api::method::thread as wire;

use crate::store::HoldStore;

/// POST /v1/users/{user}/quarantine/{thread_id}/release — it was not
/// fraud. The conversation goes back to whatever list it belonged to; the
/// verdict on the message is left alone, since it records what the rules
/// saw then. One not held is a member's no-op; one not theirs is a 404.
pub async fn release<S: HoldStore>(
    State(state): State<Arc<S>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Response {
    let fail = |e: crate::store::StoreError| {
        tracing::error!(error = %e, %user, %thread_id, "release failed");
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    };
    let held = match state.held(&user, &thread_id).await {
        Ok(Some(held)) => held,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return fail(e),
    };
    if held {
        if let Err(e) = state.release_by_person(&user, &thread_id).await {
            return fail(e);
        }
        // Logged rather than merely done: a false positive is the one
        // failure this feature can have, and a release nobody can count is
        // a failure rate nobody can measure.
        tracing::info!(%user, %thread_id, "released: a person said this was not fraud");
        state.queue_images(&user, &thread_id).await;
    }
    Json(wire::ThreadActionResponse {
        affected: 1,
        new_modseq: 0,
    })
    .into_response()
}

/// GET /v1/users/{user}/threads/{thread_id}/releases
pub async fn thread_releases<S: HoldStore>(
    State(state): State<Arc<S>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Response {
    match state.releases(&user, &thread_id).await {
        Ok(items) => Json(wire::ReleasesResponse { items }).into_response(),
        Err(e) => {
            tracing::error!(error = %e, %user, %thread_id, "reading the releases failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
