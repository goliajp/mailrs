//! Handlers for `mailrs_core_api::method::message`: one message of a
//! user's, by uid or by Message-ID, and the invitation it carries.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use mailrs_core_api::method::message as wire;

use crate::core_rpc::CoreRpcState;

/// GET /v1/users/{user}/messages/by-uid/{uid} — user-scoped uid lookup,
/// the route webapi's attachment/IMAP path calls. A uid is the user's,
/// whichever mailbox the message sits in.
pub async fn get_message_by_uid_for_user(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, uid)): Path<(String, u32)>,
) -> Result<Json<wire::MessageWire>, StatusCode> {
    let fail = |e: sqlx::Error| {
        tracing::warn!(error = %e, %user, uid, "get_message_by_uid_for_user failed");
        StatusCode::INTERNAL_SERVER_ERROR
    };
    let meta = state
        .mailbox
        .find_message_by_uid(&user, uid)
        .await
        .map_err(fail)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let row = state
        .mailbox
        .get_message(meta.mailbox_id, uid)
        .await
        .map_err(fail)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let mut w: wire::MessageWire = (&row).into();
    w.user_address = user;
    Ok(Json(w))
}

/// GET /v1/users/{user}/messages/by-uid/{uid}/invite — the invitation the
/// message carries, as stored at delivery; 404 when it carries none.
pub async fn get_invite(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, uid)): Path<(String, u32)>,
) -> Result<axum::response::Response, StatusCode> {
    use axum::response::IntoResponse;
    let payload: Option<(String,)> = sqlx::query_as(
        "SELECT CAST(m.invite_payload AS TEXT) FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
          WHERE mb.user_address = $1 AND m.uid = $2 AND m.invite_payload IS NOT NULL
          LIMIT 1",
    )
    .bind(&user)
    .bind(uid as i32)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::warn!(error = %e, %user, uid, "get_invite failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let (payload,) = payload.ok_or(StatusCode::NOT_FOUND)?;
    Ok(([("content-type", "application/json")], payload).into_response())
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

// ── flag ops (IMAP STORE) ────────────────────────────────────────────

// ── message mutate (expunge / copy / move) ──────────────────────────
