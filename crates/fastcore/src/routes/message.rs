//! Per-message IMAP ops over fastcore's stores: reads + flags come from
//! the thread-store's per-user uid index (fastcore is INBOX-centric, so a
//! mailbox_id resolves to (user, name) and the per-user uid ≈ the mailbox
//! uid); copy/move/expunge use the maildir IMAP backend. modseq is bumped
//! via the same backend counter the embedded IMAP session uses, so
//! CONDSTORE is consistent with fastcore's live IMAP.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;

use mailrs_core_api::method::message::{
    CondstoreRequest, CondstoreResponse, CopyMoveRequest, CopyMoveResponse, ExpungeResponse,
    FlagMutationRequest, FlagMutationResponse, FlagOpWire, ListMessagesQuery, ListMessagesResponse,
    MessageWire,
};

use crate::FastcoreState;
use crate::imap::backend;

fn resolve(state: &Arc<FastcoreState>, id: i64) -> Option<(String, String)> {
    state.mailbox.lookup_mailbox_id(id).ok().flatten()
}

fn load_by_uid(state: &Arc<FastcoreState>, user: &str, uid: u32) -> Option<MessageWire> {
    let bytes = state.mailbox.get_message_by_uid(user, uid).ok().flatten()?;
    serde_json::from_slice(&bytes).ok()
}

fn apply_op(cur: u32, op: FlagOpWire, flags: u32) -> u32 {
    match op {
        FlagOpWire::Set => flags,
        FlagOpWire::Add => cur | flags,
        FlagOpWire::Remove => cur & !flags,
    }
}

/// Rewrite a message's flags + bump its modseq, returning the new modseq.
fn write_flags(
    state: &Arc<FastcoreState>,
    user: &str,
    mut wire: MessageWire,
    new_flags: u32,
) -> Option<u64> {
    let new_modseq = backend::bump_modseq(state, user);
    wire.flags = new_flags;
    wire.modseq = new_modseq;
    let json = serde_json::to_vec(&wire).ok()?;
    state
        .mailbox
        .upsert_message(&wire.thread_id, &wire.message_id, wire.internal_date, &json)
        .ok()?;
    Some(new_modseq)
}

// ── reads ───────────────────────────────────────────────────────────

pub async fn get_message_by_uid(
    State(state): State<Arc<FastcoreState>>,
    Path((mailbox_id, uid)): Path<(i64, u32)>,
) -> Result<Json<MessageWire>, StatusCode> {
    let (user, _) = resolve(&state, mailbox_id).ok_or(StatusCode::NOT_FOUND)?;
    load_by_uid(&state, &user, uid)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

pub async fn find_by_message_id(
    State(state): State<Arc<FastcoreState>>,
    Path((user, message_id)): Path<(String, String)>,
) -> Result<Json<MessageWire>, StatusCode> {
    let bytes = state
        .mailbox
        .get_message(&message_id)
        .ok()
        .flatten()
        .ok_or(StatusCode::NOT_FOUND)?;
    let mut wire: MessageWire =
        serde_json::from_slice(&bytes).map_err(|_| StatusCode::NOT_FOUND)?;
    wire.user_address = user;
    Ok(Json(wire))
}

fn list_wires(state: &Arc<FastcoreState>, user: &str, name: &str) -> Vec<MessageWire> {
    let Some(mb) = backend::get_mailbox(state, user, name) else {
        return Vec::new();
    };
    backend::list_messages(state, user, &mb)
        .into_iter()
        .filter_map(|m| load_by_uid(state, user, m.uid))
        .collect()
}

pub async fn list_messages(
    State(state): State<Arc<FastcoreState>>,
    Path(mailbox_id): Path<i64>,
    Query(q): Query<ListMessagesQuery>,
) -> Json<ListMessagesResponse> {
    let Some((user, name)) = resolve(&state, mailbox_id) else {
        return Json(ListMessagesResponse { items: Vec::new() });
    };
    let items = list_wires(&state, &user, &name)
        .into_iter()
        .skip(q.offset as usize)
        .take(q.limit as usize)
        .collect();
    Json(ListMessagesResponse { items })
}

pub async fn changed_since(
    State(state): State<Arc<FastcoreState>>,
    Path((mailbox_id, modseq)): Path<(i64, u64)>,
) -> Json<ListMessagesResponse> {
    let Some((user, name)) = resolve(&state, mailbox_id) else {
        return Json(ListMessagesResponse { items: Vec::new() });
    };
    let items = list_wires(&state, &user, &name)
        .into_iter()
        .filter(|w| w.modseq > modseq)
        .collect();
    Json(ListMessagesResponse { items })
}

// ── flags / CONDSTORE ───────────────────────────────────────────────

pub async fn set_flags(
    State(state): State<Arc<FastcoreState>>,
    Path((mailbox_id, uid)): Path<(i64, u32)>,
    Json(req): Json<FlagMutationRequest>,
) -> Result<Json<FlagMutationResponse>, StatusCode> {
    let (user, _) = resolve(&state, mailbox_id).ok_or(StatusCode::NOT_FOUND)?;
    let wire = load_by_uid(&state, &user, uid).ok_or(StatusCode::NOT_FOUND)?;
    let new_flags = apply_op(wire.flags, req.op, req.flags);
    let new_modseq =
        write_flags(&state, &user, wire, new_flags).ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(FlagMutationResponse { new_modseq }))
}

pub async fn flags_if_unchanged(
    State(state): State<Arc<FastcoreState>>,
    Path((mailbox_id, uid)): Path<(i64, u32)>,
    Json(req): Json<CondstoreRequest>,
) -> Result<Json<CondstoreResponse>, StatusCode> {
    let (user, _) = resolve(&state, mailbox_id).ok_or(StatusCode::NOT_FOUND)?;
    let wire = load_by_uid(&state, &user, uid).ok_or(StatusCode::NOT_FOUND)?;
    if wire.modseq > req.unchanged_since {
        return Ok(Json(CondstoreResponse::Conflict {
            current_modseq: wire.modseq,
        }));
    }
    let new_flags = apply_op(wire.flags, req.op, req.flags);
    let new_modseq =
        write_flags(&state, &user, wire, new_flags).ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(CondstoreResponse::Applied { new_modseq }))
}

// ── copy / move / expunge (maildir backend) ─────────────────────────

fn find_imap_msg(
    state: &Arc<FastcoreState>,
    user: &str,
    name: &str,
    uid: u32,
) -> Option<crate::imap::backend::ImapMessage> {
    let mb = backend::get_mailbox(state, user, name)?;
    backend::list_messages(state, user, &mb)
        .into_iter()
        .find(|m| m.uid == uid)
}

pub async fn copy_message(
    State(state): State<Arc<FastcoreState>>,
    Path((user, src_id, uid)): Path<(String, i64, u32)>,
    Json(req): Json<CopyMoveRequest>,
) -> Result<Json<CopyMoveResponse>, StatusCode> {
    let (_, src_name) = resolve(&state, src_id).ok_or(StatusCode::NOT_FOUND)?;
    let msg = find_imap_msg(&state, &user, &src_name, uid).ok_or(StatusCode::NOT_FOUND)?;
    let dst =
        backend::get_mailbox(&state, &user, &req.dst_mailbox_name).ok_or(StatusCode::NOT_FOUND)?;
    let new_uid = backend::uid_next(&state, &user);
    backend::copy_to(&state, &user, &msg, &dst).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(CopyMoveResponse { new_uid }))
}

pub async fn move_message(
    State(state): State<Arc<FastcoreState>>,
    Path((user, src_id, uid)): Path<(String, i64, u32)>,
    Json(req): Json<CopyMoveRequest>,
) -> Result<Json<CopyMoveResponse>, StatusCode> {
    let (_, src_name) = resolve(&state, src_id).ok_or(StatusCode::NOT_FOUND)?;
    let msg = find_imap_msg(&state, &user, &src_name, uid).ok_or(StatusCode::NOT_FOUND)?;
    let dst =
        backend::get_mailbox(&state, &user, &req.dst_mailbox_name).ok_or(StatusCode::NOT_FOUND)?;
    let new_uid = backend::uid_next(&state, &user);
    backend::copy_to(&state, &user, &msg, &dst).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    // move = copy then remove the source maildir file
    let _ = std::fs::remove_file(&msg.path);
    Ok(Json(CopyMoveResponse { new_uid }))
}

pub async fn expunge(
    State(state): State<Arc<FastcoreState>>,
    Path(mailbox_id): Path<i64>,
) -> Json<ExpungeResponse> {
    let Some((user, name)) = resolve(&state, mailbox_id) else {
        return Json(ExpungeResponse {
            expunged_uids: Vec::new(),
        });
    };
    let Some(mb) = backend::get_mailbox(&state, &user, &name) else {
        return Json(ExpungeResponse {
            expunged_uids: Vec::new(),
        });
    };
    // remove maildir files flagged \Deleted (the 'T' info flag)
    let mut expunged = Vec::new();
    for m in backend::list_messages(&state, &user, &mb) {
        let is_deleted = m
            .path
            .to_str()
            .and_then(|s| s.rsplit_once(":2,"))
            .map(|(_, flags)| flags.contains('T'))
            .unwrap_or(false);
        if is_deleted && std::fs::remove_file(&m.path).is_ok() {
            expunged.push(m.uid);
        }
    }
    Json(ExpungeResponse {
        expunged_uids: expunged,
    })
}
