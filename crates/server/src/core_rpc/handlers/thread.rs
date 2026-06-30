//! Handlers for `mailrs_core_api::method::thread`.
//!
//! 12 mutate + a few read endpoints. Each thin pass-through.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use mailrs_core_api::method::thread as wire;

use crate::core_rpc::CoreRpcState;

/// GET /v1/users/{user}/threads/{thread_id}/messages
pub async fn list_thread_messages(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ListThreadMessagesResponse>, StatusCode> {
    let rows = state
        .mailbox
        .list_thread_messages(&user, &thread_id, None)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, thread_id = %thread_id, "list_thread_messages failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let user_clone = user.clone();
    let items = rows
        .iter()
        .map(|m| {
            let mut w: mailrs_core_api::method::message::MessageWire = m.into();
            w.user_address = user_clone.clone();
            w
        })
        .collect();
    Ok(Json(wire::ListThreadMessagesResponse { items }))
}

/// Helper that wraps `Result<u32, sqlx::Error>` into a `ThreadActionResponse`.
async fn into_action_response(
    res: Result<u32, sqlx::Error>,
    context: &'static str,
    user: &str,
    thread_id: &str,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    let affected = res.map_err(|e| {
        tracing::warn!(error = %e, user, thread_id, context, "thread mutate failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(wire::ThreadActionResponse {
        affected,
        new_modseq: 0,
    }))
}

/// POST /v1/users/{user}/threads/{thread_id}/read
pub async fn mark_read(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    let res = state
        .mailbox
        .mark_thread_read(&user, &thread_id, None)
        .await;
    into_action_response(res, "mark_read", &user, &thread_id).await
}

/// POST /v1/users/{user}/threads/{thread_id}/unread
pub async fn mark_unread(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    let res = state.mailbox.mark_thread_unread(&user, &thread_id).await;
    into_action_response(res, "mark_unread", &user, &thread_id).await
}

/// POST /v1/users/{user}/threads/{thread_id}/star
pub async fn star(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    let res = state.mailbox.star_thread(&user, &thread_id).await;
    into_action_response(res, "star", &user, &thread_id).await
}

/// POST /v1/users/{user}/threads/{thread_id}/unstar
pub async fn unstar(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    let res = state.mailbox.unstar_thread(&user, &thread_id).await;
    into_action_response(res, "unstar", &user, &thread_id).await
}

/// POST /v1/users/{user}/threads/{thread_id}/pin
pub async fn pin(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    let res = state.mailbox.pin_thread(&user, &thread_id).await;
    into_action_response(res, "pin", &user, &thread_id).await
}

/// POST /v1/users/{user}/threads/{thread_id}/unpin
pub async fn unpin(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    let res = state.mailbox.unpin_thread(&user, &thread_id).await;
    into_action_response(res, "unpin", &user, &thread_id).await
}

/// POST /v1/users/{user}/threads/{thread_id}/archive
pub async fn archive(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    let res = state.mailbox.archive_thread(&user, &thread_id).await;
    into_action_response(res, "archive", &user, &thread_id).await
}

/// POST /v1/users/{user}/threads/{thread_id}/unarchive
pub async fn unarchive(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    let res = state.mailbox.unarchive_thread(&user, &thread_id).await;
    into_action_response(res, "unarchive", &user, &thread_id).await
}

/// POST /v1/users/{user}/threads/{thread_id}/dismiss-action
pub async fn dismiss_action(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    let res = state.mailbox.dismiss_thread_action(&user, &thread_id).await;
    into_action_response(res, "dismiss_action", &user, &thread_id).await
}

/// PUT /v1/users/{user}/threads/{thread_id}/snooze
pub async fn snooze(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
    Json(req): Json<wire::SnoozeRequest>,
) -> Result<StatusCode, StatusCode> {
    use chrono::TimeZone;
    let until = chrono::Utc
        .timestamp_opt(req.snoozed_until, 0)
        .single()
        .ok_or(StatusCode::BAD_REQUEST)?;
    state
        .mailbox
        .snooze_thread(&user, &thread_id, until)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, thread_id = %thread_id, "snooze failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /v1/users/{user}/threads/{thread_id}/snooze
pub async fn unsnooze(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<StatusCode, StatusCode> {
    state
        .mailbox
        .unsnooze_thread(&user, &thread_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, thread_id = %thread_id, "unsnooze failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /v1/users/{user}/threads/{thread_id}
pub async fn delete_thread(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<StatusCode, StatusCode> {
    state
        .mailbox
        .delete_thread(&user, &thread_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, thread_id = %thread_id, "delete_thread failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}
