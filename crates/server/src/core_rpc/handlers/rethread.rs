//! Operator repairs of which conversation a message belongs to.

use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode};

use mailrs_core_api::method::rethread as wire;

use crate::core_rpc::CoreRpcState;

/// POST /v1/admin/threads:split-message — move one of a user's messages
/// into a conversation of its own. 404 when the user holds no such message.
pub async fn split_message(
    State(state): State<Arc<CoreRpcState>>,
    Json(req): Json<wire::SplitMessageRequest>,
) -> Result<Json<wire::SplitMessageResponse>, StatusCode> {
    if req.user.is_empty() || req.message_id.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let moved = state
        .mailbox
        .split_message(&req.user, &req.message_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %req.user, mid = %req.message_id, "split_message failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let thread_id = moved.ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(wire::SplitMessageResponse { thread_id }))
}
