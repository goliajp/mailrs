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

/// GET /v1/mailboxes/{id}/messages/uid/{uid}/raw
///
/// Streams the raw RFC 5322 bytes from disk. Resolves mailbox owner
/// via mailbox_id → mailbox.user_address, then opens
/// `{maildir_root}/{user}/cur|new/{maildir_id}`.
pub async fn get_message_raw(
    State(state): State<Arc<CoreRpcState>>,
    Path((mailbox_id, uid)): Path<(i64, u32)>,
) -> Result<Vec<u8>, StatusCode> {
    let meta = state
        .mailbox
        .get_message(mailbox_id, uid)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, mailbox_id, uid, "get_message_raw lookup failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Need user_address to construct the maildir path — meta only has
    // mailbox_id, so resolve through mailbox row.
    let mbox = state
        .mailbox
        .get_mailbox_by_id(mailbox_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, mailbox_id, "mailbox lookup failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    // maildir layout: {root}/{user}/{cur|new}/{maildir_id}
    let cur = std::path::Path::new(&state.maildir_root)
        .join(&mbox.user)
        .join("cur")
        .join(&meta.maildir_id);
    let new_path = std::path::Path::new(&state.maildir_root)
        .join(&mbox.user)
        .join("new")
        .join(&meta.maildir_id);
    let path = if cur.exists() { cur } else { new_path };
    tokio::fs::read(&path).await.map_err(|e| {
        tracing::warn!(error = %e, path = %path.display(), "raw read failed");
        StatusCode::NOT_FOUND
    })
}

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

/// GET /v1/users/{user}/messages/by-uid/{uid} — user-scoped uid lookup,
/// the route webapi's attachment/IMAP path calls. fastcore serves this
/// natively via its per-user uid index; the PG core resolves the user's
/// INBOX then reads by uid, so both cores answer identical requests.
pub async fn get_message_by_uid_for_user(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, uid)): Path<(String, u32)>,
) -> Result<Json<wire::MessageWire>, StatusCode> {
    let mb = state
        .mailbox
        .get_mailbox(&user, "INBOX")
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let row = state
        .mailbox
        .get_message(mb.id, uid)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, uid, "get_message_by_uid_for_user failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;
    let mut w: wire::MessageWire = (&row).into();
    w.user_address = user;
    Ok(Json(w))
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

// ── flag ops (IMAP STORE) ────────────────────────────────────────────

/// PUT /v1/mailboxes/{id}/messages/{uid}/flags  — Set / Add / Remove via op
pub async fn flag_mutation(
    State(state): State<Arc<CoreRpcState>>,
    Path((mailbox_id, uid)): Path<(i64, u32)>,
    Json(req): Json<wire::FlagMutationRequest>,
) -> Result<Json<wire::FlagMutationResponse>, StatusCode> {
    let new_modseq = match req.op {
        wire::FlagOpWire::Set => state.mailbox.update_flags(mailbox_id, uid, req.flags).await,
        wire::FlagOpWire::Add => state.mailbox.add_flags(mailbox_id, uid, req.flags).await,
        wire::FlagOpWire::Remove => state.mailbox.remove_flags(mailbox_id, uid, req.flags).await,
    }
    .map_err(|e| {
        tracing::warn!(error = %e, mailbox_id, uid, ?req.op, "flag mutation failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(wire::FlagMutationResponse { new_modseq }))
}

/// POST /v1/mailboxes/{id}/messages/{uid}/condstore  — RFC 7162 CAS
pub async fn condstore(
    State(state): State<Arc<CoreRpcState>>,
    Path((mailbox_id, uid)): Path<(i64, u32)>,
    Json(req): Json<wire::CondstoreRequest>,
) -> Json<wire::CondstoreResponse> {
    let action = req.op.into();
    match state
        .mailbox
        .update_flags_if_unchanged(mailbox_id, uid, req.flags, action, req.unchanged_since)
        .await
    {
        Ok(Some(new_modseq)) => Json(wire::CondstoreResponse::Applied { new_modseq }),
        Ok(None) => Json(wire::CondstoreResponse::Conflict {
            current_modseq: req.unchanged_since,
        }),
        Err(e) => {
            tracing::warn!(error = %e, mailbox_id, uid, "condstore failed");
            // Map sqlx errors to a conflict with modseq=0 sentinel so the
            // client retries / displays the error. A richer error model
            // lands in checklist 2.5.
            Json(wire::CondstoreResponse::Conflict { current_modseq: 0 })
        }
    }
}

/// GET /v1/mailboxes/{id}/changed-since/{modseq}
pub async fn changed_since(
    State(state): State<Arc<CoreRpcState>>,
    Path((mailbox_id, modseq)): Path<(i64, u64)>,
) -> Result<Json<wire::ListMessagesResponse>, StatusCode> {
    let rows = state
        .mailbox
        .list_messages_changed_since(mailbox_id, modseq)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, mailbox_id, modseq, "changed_since failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let items = rows.iter().map(Into::into).collect();
    Ok(Json(wire::ListMessagesResponse { items }))
}

// ── message mutate (expunge / copy / move) ──────────────────────────

/// POST /v1/mailboxes/{id}/expunge  — IMAP EXPUNGE
pub async fn expunge(
    State(state): State<Arc<CoreRpcState>>,
    Path(mailbox_id): Path<i64>,
) -> Result<Json<wire::ExpungeResponse>, StatusCode> {
    let expunged_uids = state.mailbox.expunge(mailbox_id).await.map_err(|e| {
        tracing::warn!(error = %e, mailbox_id, "expunge failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(wire::ExpungeResponse { expunged_uids }))
}

/// POST /v1/users/{user}/mailboxes/{src_id}/messages/{uid}/copy
pub async fn copy_message(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, src_mailbox_id, uid)): Path<(String, i64, u32)>,
    Json(req): Json<wire::CopyMoveRequest>,
) -> Result<Json<wire::CopyMoveResponse>, StatusCode> {
    let new_uid = state
        .mailbox
        .copy_message(&user, src_mailbox_id, uid, &req.dst_mailbox_name)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, src_mailbox_id, uid, dst = %req.dst_mailbox_name, "copy failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(wire::CopyMoveResponse { new_uid }))
}

/// POST /v1/users/{user}/mailboxes/{src_id}/messages/{uid}/move
pub async fn move_message(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, src_mailbox_id, uid)): Path<(String, i64, u32)>,
    Json(req): Json<wire::CopyMoveRequest>,
) -> Result<Json<wire::CopyMoveResponse>, StatusCode> {
    let new_uid = state
        .mailbox
        .move_message(&user, src_mailbox_id, uid, &req.dst_mailbox_name)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, src_mailbox_id, uid, dst = %req.dst_mailbox_name, "move failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(wire::CopyMoveResponse { new_uid }))
}
