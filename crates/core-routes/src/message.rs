//! Reading a user's messages: a conversation's, one by uid or Message-ID,
//! and the invitation one carries. Each answer names the user it is for.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_core_api::method::message::MessageWire;
use mailrs_core_api::method::thread::ListThreadMessagesResponse;

use crate::store::{MessageReads, StoreResult};

fn failed(e: crate::store::StoreError, what: &str, user: &str) -> Response {
    tracing::warn!(error = %e, %user, what, "message read failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// One message for `user`: their address on it, 404 when they hold none.
fn one(found: StoreResult<Option<MessageWire>>, what: &str, user: String) -> Response {
    match found {
        Ok(Some(w)) => Json(MessageWire {
            user_address: user,
            ..w
        })
        .into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => failed(e, what, &user),
    }
}

/// GET /v1/users/{user}/threads/{thread_id}/messages
pub async fn thread_messages<S: MessageReads>(
    State(state): State<Arc<S>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Response {
    match state.thread_messages(&user, &thread_id).await {
        Ok(items) => Json(ListThreadMessagesResponse {
            items: items
                .into_iter()
                .map(|w| MessageWire {
                    user_address: user.clone(),
                    ..w
                })
                .collect(),
        })
        .into_response(),
        Err(e) => failed(e, "thread_messages", &user),
    }
}

/// GET /v1/users/{user}/messages/by-uid/{uid}
pub async fn message_by_uid<S: MessageReads>(
    State(state): State<Arc<S>>,
    Path((user, uid)): Path<(String, u32)>,
) -> Response {
    one(state.message_by_uid(&user, uid).await, "by_uid", user)
}

/// GET /v1/users/{user}/messages/by-message-id/{message_id}
pub async fn message_by_message_id<S: MessageReads>(
    State(state): State<Arc<S>>,
    Path((user, message_id)): Path<(String, String)>,
) -> Response {
    one(
        state.message_by_message_id(&user, &message_id).await,
        "by_message_id",
        user,
    )
}

/// GET /v1/users/{user}/messages/by-uid/{uid}/invite — 404 when the message
/// carries none, which is nearly all mail.
pub async fn invite<S: MessageReads>(
    State(state): State<Arc<S>>,
    Path((user, uid)): Path<(String, u32)>,
) -> Response {
    match state.invite(&user, uid).await {
        Ok(Some(payload)) => ([("content-type", "application/json")], payload).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => failed(e, "invite", &user),
    }
}
