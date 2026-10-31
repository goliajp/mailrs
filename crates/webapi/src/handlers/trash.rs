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
    delete_for_good(&state, user, thread_id).await
}

/// Delete `thread_id` for good, with the Send rows for the mail in it.
pub(crate) async fn delete_for_good(
    state: &WebState,
    user: String,
    thread_id: String,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .delete_thread(&user, &thread_id)
        .await
        .map_err(map_err)?;
    // The Send rows for the mail in it go too, or the Send list keeps
    // showing them with nothing behind.
    crate::handlers::kevy_util::with_kevy(move |c| {
        mailrs_core_sidestate::families::send::delete_sends_in_thread(c, &user, &thread_id)
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/trash/empty — everything in the Trash, for good.
pub async fn empty_trash(
    _: crate::session::confirm::Confirmed,
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let done = state.core.empty_trash(&user).await.map_err(map_err)?;
    Ok(Json(serde_json::json!({ "deleted": done.affected })))
}
