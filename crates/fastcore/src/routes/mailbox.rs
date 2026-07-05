//! Mailbox CRUD over the maildir IMAP backend fastcore already runs, so
//! the core-api mailbox routes reuse the working uid/modseq machinery
//! rather than a second kevy model. The contract keys mailboxes by an i64
//! `MailboxId`; we bridge via the stable `mailbox-kevy::mbid` index so
//! bare-id routes (get_mailbox_by_id / mailbox_status) resolve back to
//! (user, name).

use std::path::PathBuf;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use mailrs_core_api::method::mailbox::{
    CreateMailboxRequest, CreateMailboxResponse, MailboxStatusResponse, MailboxStatusWire,
    MailboxWire, RenameMailboxRequest,
};
use mailrs_mailbox_kevy::mbid::mailbox_id;

use crate::FastcoreState;
use crate::imap::backend;

/// `user@domain` → the maildir base dir `{root}/{domain}/{local}`.
fn user_base(user: &str) -> PathBuf {
    let root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let (local, domain) = user.split_once('@').unwrap_or((user, "localhost"));
    PathBuf::from(root).join(domain).join(local)
}

/// Maildir path for a mailbox name (INBOX = base; else base/name).
fn mailbox_path(user: &str, name: &str) -> PathBuf {
    let base = user_base(user);
    if name.eq_ignore_ascii_case("INBOX") {
        base
    } else {
        base.join(name)
    }
}

fn to_wire(state: &Arc<FastcoreState>, user: &str, name: &str) -> MailboxWire {
    let _ = state.mailbox.register_mailbox_id(user, name);
    MailboxWire {
        id: mailbox_id(user, name),
        user: user.to_string(),
        name: name.to_string(),
        uidvalidity: backend::uidvalidity(state, user, name),
        uidnext: backend::uid_next(state, user),
        highest_modseq: backend::highest_modseq(state, user),
    }
}

pub async fn get_mailbox(
    State(state): State<Arc<FastcoreState>>,
    Path((user, name)): Path<(String, String)>,
) -> Result<Json<MailboxWire>, StatusCode> {
    match backend::get_mailbox(&state, &user, &name) {
        Some(_) => Ok(Json(to_wire(&state, &user, &name))),
        None => Err(StatusCode::NOT_FOUND),
    }
}

pub async fn get_mailbox_by_id(
    State(state): State<Arc<FastcoreState>>,
    Path(id): Path<i64>,
) -> Result<Json<MailboxWire>, StatusCode> {
    let (user, name) = state
        .mailbox
        .lookup_mailbox_id(id)
        .ok()
        .flatten()
        .ok_or(StatusCode::NOT_FOUND)?;
    match backend::get_mailbox(&state, &user, &name) {
        Some(_) => Ok(Json(to_wire(&state, &user, &name))),
        None => Err(StatusCode::NOT_FOUND),
    }
}

pub async fn create_mailbox(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
    Json(req): Json<CreateMailboxRequest>,
) -> Result<Json<CreateMailboxResponse>, StatusCode> {
    let path = mailbox_path(&user, &req.name);
    for sub in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(path.join(sub)).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    Ok(Json(CreateMailboxResponse {
        mailbox: to_wire(&state, &user, &req.name),
    }))
}

pub async fn delete_mailbox(
    State(state): State<Arc<FastcoreState>>,
    Path((user, name)): Path<(String, String)>,
) -> StatusCode {
    if name.eq_ignore_ascii_case("INBOX") {
        return StatusCode::FORBIDDEN; // INBOX is not deletable (RFC 3501)
    }
    let path = mailbox_path(&user, &name);
    if !path.exists() {
        return StatusCode::NOT_FOUND;
    }
    match std::fs::remove_dir_all(&path) {
        Ok(_) => {
            let _ = state.mailbox.forget_mailbox_id(mailbox_id(&user, &name));
            StatusCode::NO_CONTENT
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub async fn rename_mailbox(
    State(state): State<Arc<FastcoreState>>,
    Path((user, name)): Path<(String, String)>,
    Json(req): Json<RenameMailboxRequest>,
) -> StatusCode {
    let from = mailbox_path(&user, &name);
    let to = mailbox_path(&user, &req.to);
    if !from.exists() {
        return StatusCode::NOT_FOUND;
    }
    match std::fs::rename(&from, &to) {
        Ok(_) => {
            let _ = state.mailbox.forget_mailbox_id(mailbox_id(&user, &name));
            let _ = state.mailbox.register_mailbox_id(&user, &req.to);
            StatusCode::NO_CONTENT
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub async fn mailbox_status(
    State(state): State<Arc<FastcoreState>>,
    Path(id): Path<i64>,
) -> Result<Json<MailboxStatusResponse>, StatusCode> {
    let (user, name) = state
        .mailbox
        .lookup_mailbox_id(id)
        .ok()
        .flatten()
        .ok_or(StatusCode::NOT_FOUND)?;
    let path = mailbox_path(&user, &name);
    let count_dir = |sub: &str| -> u32 {
        std::fs::read_dir(path.join(sub))
            .map(|it| it.filter_map(|e| e.ok()).count() as u32)
            .unwrap_or(0)
    };
    let cur = count_dir("cur");
    let new = count_dir("new");
    // maildir `new` = unseen + recent; `cur` = already-seen. Total = both.
    Ok(Json(MailboxStatusResponse {
        status: MailboxStatusWire {
            total: cur + new,
            unread: new,
            recent: new,
        },
    }))
}
