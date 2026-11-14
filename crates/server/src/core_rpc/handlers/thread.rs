//! Handlers for `mailrs_core_api::method::thread`.
//!
//! 12 mutate + a few read endpoints. Each thin pass-through.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
};

use crate::core_rpc::CoreRpcState;

/// Helper that wraps `Result<u32, sqlx::Error>` into a `ThreadActionResponse`.
/// 404 unless `user` is in the thread: a thread somebody else holds and
/// one that does not exist answer alike, so the answer cannot be used to
/// find out which ids exist.
pub(crate) async fn require_member(
    state: &CoreRpcState,
    user: &str,
    thread_id: &str,
) -> Result<(), StatusCode> {
    let member = state
        .mailbox
        .user_in_thread(user, thread_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user, thread_id, "membership check failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    match member {
        true => Ok(()),
        false => Err(StatusCode::NOT_FOUND),
    }
}

/// DELETE /v1/users/{user}/threads/{thread_id}
pub async fn delete_thread(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<StatusCode, StatusCode> {
    delete_for_good(&state, &user, &thread_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// What deleting a conversation for good took off the disk.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Deleted {
    pub messages: usize,
    pub bytes: u64,
}

/// Delete `user`'s `thread_id` for good, rows and files; 404 when it is
/// not the user's.
pub(crate) async fn delete_for_good(
    state: &CoreRpcState,
    user: &str,
    thread_id: &str,
) -> Result<Deleted, StatusCode> {
    require_member(state, user, thread_id).await?;
    let files = state
        .mailbox
        .delete_thread(user, thread_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, thread_id = %thread_id, "delete_thread failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    // Every copy's file goes too: one left on disk is a message the user
    // deleted and still has, and the self-heal of either core files it
    // again.
    let mut gone = Deleted::default();
    let mut left = 0usize;
    let net = state.net_url.as_deref();
    for file in &files {
        match crate::body::remove(net, &state.maildir_root, user, file) {
            Ok(Some(size)) => {
                gone.messages += 1;
                gone.bytes += size;
            }
            Ok(None) => {}
            Err(e) => {
                tracing::error!(error = %e, %user, %file,
                    "delete_thread: a file stayed on disk; self-heal will file it again");
                left += 1;
            }
        }
    }
    // the conversation comes back with the next self-heal, so the delete
    // did not happen and the caller is told so
    if left > 0 {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    tracing::info!(%user, %thread_id, files = files.len(), unlinked = gone.messages, "delete_thread");
    Ok(gone)
}
