//! Handlers for `mailrs_core_api::method::message`.
//!
//! Phase 2.2 subset — read-side endpoints first (IMAP/JMAP/web hot path):
//! - GET    /v1/mailboxes/{id}/messages/uid/{uid}     (get by mailbox+uid)
//! - GET    /v1/mailboxes/{id}/messages               (list paginated)
//! - GET    /v1/messages/{id}                         (get by db id — JMAP)
//! - POST   /v1/users/{user}/messages:query           (JMAP Email/query)
//! - GET    /v1/users/{user}/messages/by-message-id/{message_id}
//!
//! Mutate / flag endpoints land in subsequent loops.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};

use mailrs_core_api::method::message as wire;

use crate::core_rpc::CoreRpcState;

/// GET /v1/mailboxes/{id}/messages/uid/{uid}
pub async fn get_message_by_uid(
    State(state): State<Arc<CoreRpcState>>,
    Path((mailbox_id, uid)): Path<(i64, u32)>,
) -> Result<Json<wire::MessageWire>, StatusCode> {
    let row = state
        .mailbox
        .get_message(mailbox_id, uid)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, mailbox_id, uid, "get_message failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json((&row).into()))
}

/// GET /v1/mailboxes/{id}/messages?offset=&limit=
pub async fn list_messages(
    State(state): State<Arc<CoreRpcState>>,
    Path(mailbox_id): Path<i64>,
    Query(q): Query<wire::ListMessagesQuery>,
) -> Result<Json<wire::ListMessagesResponse>, StatusCode> {
    let rows = state
        .mailbox
        .list_messages(mailbox_id, q.offset, q.limit)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, mailbox_id, "list_messages failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let items = rows.iter().map(Into::into).collect();
    Ok(Json(wire::ListMessagesResponse { items }))
}

/// GET /v1/users/{user}/messages/by-message-id/{message_id}
pub async fn find_message_by_message_id(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, message_id)): Path<(String, String)>,
) -> Result<Json<wire::MessageWire>, StatusCode> {
    let row = state
        .mailbox
        .find_message_by_message_id(&user, &message_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, message_id = %message_id, "find by message-id failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;
    // MessageMeta → MessageWire, then fill in user_address from path.
    let mut wire: wire::MessageWire = (&row).into();
    wire.user_address = user;
    Ok(Json(wire))
}

// query_messages handler omitted from this loop — inherent method
// returns (Vec<i64>, u32 total), not Vec<MessageMeta>. Needs a separate
// wire response shape (id list + total) — implemented in next loop.
