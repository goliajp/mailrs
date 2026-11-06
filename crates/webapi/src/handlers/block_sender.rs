//! Block the sender of a conversation, from the message itself.
//!
//! One call does what the reader asked for in one tap: the address goes
//! on their blacklist (and off their whitelist, see `spam_lists`) and the
//! conversation goes to Junk. Later mail from the address is filed in
//! Junk on arrival; nothing is rejected and the sender is not told.
//!
//! The conversation moves first: a thread id that is not the caller's
//! answers 404 before any list is written.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use mailrs_core_sidestate::families::sender_lists::blacklist_key;

use crate::WebState;
use crate::handlers::conversations::{AuthedUser, map_err};
use crate::handlers::spam_lists::{put_on_list, remove_from_set};

#[derive(Debug, serde::Deserialize)]
pub struct BlockSenderRequest {
    /// The `From:` address of the message the reader blocked from.
    pub address: String,
}

#[derive(Debug, serde::Serialize)]
pub struct BlockSenderResponse {
    /// As stored: trimmed and lowercased.
    pub address: String,
    /// It was on the whitelist and is not any more. An undo sends it
    /// back as `allow`.
    pub was_allowed: bool,
}

#[derive(Debug, serde::Deserialize)]
pub struct UnblockSenderRequest {
    pub address: String,
    /// Put the address back on the whitelist, where blocking took it from.
    #[serde(default)]
    pub allow: bool,
}

/// POST /api/conversations/{thread_id}/block-sender
pub async fn block_sender(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
    Json(req): Json<BlockSenderRequest>,
) -> Result<Json<BlockSenderResponse>, StatusCode> {
    let address = req.address.trim().to_lowercase();
    // checked before the move, which it would otherwise follow
    if address == user.to_lowercase() {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    state
        .core
        .mark_junk(&user, &thread_id)
        .await
        .map_err(map_err)?;
    let was_allowed = put_on_list(&user, &address, true)?;
    Ok(Json(BlockSenderResponse {
        address,
        was_allowed,
    }))
}

/// POST /api/conversations/{thread_id}/unblock-sender — the undo of
/// `block-sender`: the address comes off the blacklist and the
/// conversation goes back to the Inbox.
///
/// The move is the core's, not the `mark-not-junk` route's, which would
/// also whitelist every sender of the thread — more than the reader asked
/// to take back.
pub async fn unblock_sender(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
    Json(req): Json<UnblockSenderRequest>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .mark_not_junk(&user, &thread_id)
        .await
        .map_err(map_err)?;
    match req.allow {
        true => {
            put_on_list(&user, &req.address, false)?;
        }
        false => remove_from_set(&blacklist_key(&user), req.address.trim())?,
    }
    Ok(StatusCode::NO_CONTENT)
}
