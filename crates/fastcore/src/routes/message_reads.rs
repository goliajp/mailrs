//! Message-level reads: a thread's messages, one message by UID, and the
//! invitation a message carries.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};

use crate::*;

/// `GET /v1/users/{user}/threads/{thread_id}/messages` — this user's copy
/// of each message in the thread.
pub(crate) async fn thread_messages(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Json<mailrs_core_api::method::thread::ListThreadMessagesResponse> {
    use mailrs_core_api::method::message::MessageWire;
    let blobs = state
        .mailbox
        .list_thread_messages(&user, &thread_id)
        .unwrap_or_default();
    // the owner is filled from the path: the stored blob is shared between
    // recipients, and a reader such as JMAP finds the file by it
    let items = blobs
        .into_iter()
        .filter_map(|b| serde_json::from_slice::<MessageWire>(&b).ok())
        .map(|w| MessageWire {
            user_address: user.clone(),
            ..w
        })
        .collect();
    Json(mailrs_core_api::method::thread::ListThreadMessagesResponse { items })
}

/// `GET /v1/users/{user}/messages/by-uid/{uid}` — look up a message by
/// the user-scoped uid index (populated by `deliver_message` /
/// `mailrs-fastcore-backfill-uid-index`). Returns the JSON MessageWire.
pub(crate) async fn get_message_by_uid_for_user(
    State(state): State<Arc<FastcoreState>>,
    Path((user, uid)): Path<(String, u32)>,
) -> Result<axum::response::Response, axum::http::StatusCode> {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use mailrs_core_api::method::message::MessageWire;
    match state.mailbox.get_message_by_uid(&user, uid) {
        Ok(Some(bytes)) => {
            let mut wire: MessageWire = serde_json::from_slice(&bytes).map_err(|e| {
                tracing::warn!(error = %e, %user, %uid, "get_message_by_uid: undecodable row");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
            wire.user_address = user;
            Ok(Json(wire).into_response())
        }
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::warn!(error = %e, %user, %uid, "get_message_by_uid failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// `GET /v1/users/{user}/messages/by-uid/{uid}/invite` — the typed
/// invitation the message carries, stored at ingest.
///
/// 404 when the message carries none, which is nearly all mail. The
/// row's `invite_method` already told the caller whether to ask, so a
/// 404 here means the two disagree — worth noticing rather than
/// answering `null`.
pub(crate) async fn get_invite(
    State(state): State<Arc<FastcoreState>>,
    Path((user, uid)): Path<(String, u32)>,
) -> Result<axum::response::Response, axum::http::StatusCode> {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    let bytes = state
        .mailbox
        .get_message_by_uid(&user, uid)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let wire: mailrs_core_api::method::message::MessageWire =
        serde_json::from_slice(&bytes).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let payload =
        crate::invites::payload_json(&state, &wire.message_id).ok_or(StatusCode::NOT_FOUND)?;
    Ok(([("content-type", "application/json")], payload).into_response())
}
