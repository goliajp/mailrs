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
    fill_attachments(&state, std::slice::from_mut(&mut w)).await?;
    Ok(Json(w))
}

/// The attachment count each message was stored with, which the message
/// rows these reads come from do not select.
pub(crate) async fn fill_attachments(
    state: &CoreRpcState,
    items: &mut [wire::MessageWire],
) -> Result<(), StatusCode> {
    let ids: Vec<i64> = items.iter().map(|w| w.id).collect();
    let counts = state.mailbox.attachment_counts(&ids).await.map_err(|e| {
        tracing::warn!(error = %e, "attachment counts failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    for w in items {
        w.attachments = counts.get(&w.id).copied();
    }
    Ok(())
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
    let mut w: wire::MessageWire = (&row).into();
    w.user_address = user;
    fill_attachments(&state, std::slice::from_mut(&mut w)).await?;
    Ok(Json(w))
}

// query_messages handler omitted from this loop — inherent method
// returns (Vec<i64>, u32 total), not Vec<MessageMeta>. Needs a separate
// wire response shape (id list + total) — implemented in next loop.

// ── flag ops (IMAP STORE) ────────────────────────────────────────────

// ── message mutate (expunge / copy / move) ──────────────────────────

/// GET /v1/users/{user}/messages/index — every message the user has, in the
/// compact form a JMAP server compares to see what changed.
pub async fn message_index(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<axum::response::Response, StatusCode> {
    use mailrs_core_api::method::message_index::{Entry, MessageIndex, respond};
    use mailrs_core_api::types::ConversationSummaryWire;
    use mailrs_mailbox::pg::{Held, ThreadQuery};
    let fail = |e: sqlx::Error| {
        tracing::warn!(error = %e, %user, "message_index failed");
        StatusCode::INTERNAL_SERVER_ERROR
    };
    let q = ThreadQuery {
        archived: None,
        trashed: None,
        held: Held::Hide,
        ..ThreadQuery::all(&user)
    };
    let rows = state.mailbox.thread_rows(&q).await.map_err(fail)?;
    let convs: Vec<ConversationSummaryWire> = rows.iter().map(Into::into).collect();
    let convs = super::labels::with_labels(&state, &user, convs).await;
    let msgs = state
        .mailbox
        .user_message_index(&user)
        .await
        .map_err(fail)?;
    let entries: Vec<Entry<'_>> = msgs
        .iter()
        .map(|(uid, tid, flags, sender)| Entry {
            uid: *uid,
            thread_id: tid,
            flags: *flags,
            own: mailrs_rfc5322::list_contains(sender, &user),
        })
        .collect();
    Ok(respond(
        &MessageIndex::build(&convs, &entries).encode(),
        &headers,
    ))
}
