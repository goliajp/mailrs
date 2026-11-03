//! The Trash, for the web and the apps: deleting moves a conversation
//! there, and it can be restored; deleting for good and emptying the
//! Trash ask for a recent password confirmation, since they cannot be
//! undone.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};

use mailrs_core_sidestate::families::audit::{self, Credential};

use crate::WebState;
use crate::handlers::conversations::{AuthedUser, map_err};

/// DELETE /api/conversations/{thread_id} — to the Trash. Every client
/// that deleted before (the web, the apps, MCP) now deletes this way.
pub async fn trash_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .trash_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/untrash
pub async fn untrash_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .untrash_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// DELETE /api/conversations/{thread_id}/forever — the files go.
pub async fn delete_forever(
    _: crate::session::confirm::Confirmed,
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let messages = message_count(&state, &user, &thread_id).await;
    delete_for_good(
        &state,
        user,
        thread_id,
        messages,
        "web",
        Credential::Session,
    )
    .await
}

/// How many messages `thread_id` holds, for the audit row; 0 when the core
/// does not say.
pub(crate) async fn message_count(state: &WebState, user: &str, thread_id: &str) -> usize {
    let req = mailrs_core_api::method::conversation::ConversationsByIdsRequest {
        thread_ids: vec![thread_id.to_string()],
        folder: None,
    };
    match state.core.conversations_by_thread_ids(user, &req).await {
        Ok(r) => r.items.first().map_or(0, |c| c.message_count as usize),
        Err(e) => {
            tracing::warn!(error = %e, "message count for the audit row unreadable");
            0
        }
    }
}

/// Delete `thread_id` (`messages` messages) for good, with the Send rows
/// for the mail in it, and write the audit row every way of deleting for
/// good writes: through `via`, signed in by `credential`.
pub(crate) async fn delete_for_good(
    state: &WebState,
    user: String,
    thread_id: String,
    messages: usize,
    via: &'static str,
    credential: Credential,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .delete_thread(&user, &thread_id)
        .await
        .map_err(map_err)?;
    // The Send rows for the mail in it go too, or the Send list keeps
    // showing them with nothing behind.
    let now = crate::handlers::complete::now_secs();
    crate::handlers::kevy_util::with_kevy(move |c| {
        mailrs_core_sidestate::families::send::delete_sends_in_thread(c, &user, &thread_id)?;
        audit::deleted_forever(c, &user, &thread_id, messages, via, credential, now)
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/trash/empty — everything in the Trash, for good. One audit
/// row for the lot: the core empties it without listing what it held.
pub async fn empty_trash(
    _: crate::session::confirm::Confirmed,
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let done = state.core.empty_trash(&user).await.map_err(map_err)?;
    let now = crate::handlers::complete::now_secs();
    let detail = format!(
        "conversations={} via=web credential={}",
        done.affected,
        Credential::Session.as_str()
    );
    crate::handlers::kevy_util::with_kevy(move |c| {
        audit::append(c, &user, audit::DELETED_FOREVER, "(trash)", &detail, now)
    })?;
    Ok(Json(serde_json::json!({ "deleted": done.affected })))
}
