//! Conversations nobody holds. Every conversation here is made of messages
//! that each belong to a mailbox, so there are none: the list is empty, a
//! conversation that exists is somebody's (409), and any other is missing.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use mailrs_core_api::method::admin as wire;

use crate::core_rpc::CoreRpcState;

/// GET /v1/admin/ownerless-threads
pub async fn ownerless_threads() -> Json<wire::OwnerlessThreadsResponse> {
    Json(wire::OwnerlessThreadsResponse { items: Vec::new() })
}

/// POST /v1/admin/ownerless-threads/{thread_id}/delete
pub async fn delete_ownerless_thread(
    State(state): State<Arc<CoreRpcState>>,
    Path(thread_id): Path<String>,
) -> StatusCode {
    let found: Result<Option<(i32,)>, _> =
        sqlx::query_as("SELECT 1 FROM messages WHERE thread_id = $1 LIMIT 1")
            .bind(&thread_id)
            .fetch_optional(&state.pool)
            .await;
    match found {
        Ok(Some(_)) => StatusCode::CONFLICT,
        Ok(None) => StatusCode::NOT_FOUND,
        Err(e) => {
            tracing::warn!(error = %e, "delete_ownerless_thread failed");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}
