//! A user's copy of one message, found by its Message-ID.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use mailrs_core_api::method::message::MessageWire;

use crate::FastcoreState;

pub async fn find_by_message_id(
    State(state): State<Arc<FastcoreState>>,
    Path((user, message_id)): Path<(String, String)>,
) -> Result<Json<MessageWire>, StatusCode> {
    // This user's copy, not the shared blob. The blob is stripped of
    // `blob_ref`, `uid`, `flags` and `modseq` on every write — they depend
    // on who is asking — so serving it reported "no file, no uid" for rows
    // that were correct, and a blank-body investigation spent three wrong
    // turns on that answer. `user_message_view` is the one decision about
    // what a user's copy is; the thread listing and the uid fetch already
    // go through it.
    let bytes = state
        .mailbox
        .user_message_view(&user, &message_id)
        .ok()
        .flatten()
        .ok_or(StatusCode::NOT_FOUND)?;
    let mut wire: MessageWire =
        serde_json::from_slice(&bytes).map_err(|_| StatusCode::NOT_FOUND)?;
    wire.user_address = user;
    Ok(Json(wire))
}
