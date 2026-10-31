//! The Trash, as fastcore keeps it: move a conversation there and back,
//! empty it, and purge what has been there 30 days. Deleting for good is
//! `thread::delete_thread`, the one place that unlinks.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use mailrs_core_api::method::thread as wire;

use super::thread::require_member;
use crate::core_rpc::CoreRpcState;

/// How long a conversation stays in the Trash.
pub(crate) const KEEP_SECS: i64 = 30 * 86_400;

fn fail(e: sqlx::Error) -> StatusCode {
    tracing::warn!(error = %e, "trash failed");
    StatusCode::INTERNAL_SERVER_ERROR
}

fn one() -> Json<wire::ThreadActionResponse> {
    Json(wire::ThreadActionResponse {
        affected: 1,
        new_modseq: 0,
    })
}

/// POST /v1/users/{user}/threads/{thread_id}/trash
pub async fn trash(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    require_member(&state, &user, &thread_id).await?;
    let now = chrono::Utc::now().timestamp();
    state
        .mailbox
        .trash_thread(&user, &thread_id, now)
        .await
        .map_err(fail)?;
    Ok(one())
}

/// POST /v1/users/{user}/threads/{thread_id}/untrash
pub async fn untrash(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    require_member(&state, &user, &thread_id).await?;
    state
        .mailbox
        .untrash_thread(&user, &thread_id)
        .await
        .map_err(fail)?;
    Ok(one())
}

async fn delete_for_good(state: &Arc<CoreRpcState>, user: &str, thread_id: &str) -> bool {
    let deleted = super::thread::delete_thread(
        State(state.clone()),
        Path((user.to_string(), thread_id.to_string())),
    )
    .await
    .is_ok();
    // the Send rows for the mail in it go too, as fastcore's do
    if deleted && let Some(mut c) = mailrs_core_sidestate::NetKevy::net_conn(state.as_ref()) {
        if let Err(e) =
            mailrs_core_sidestate::families::send::delete_sends_in_thread(&mut c, user, thread_id)
        {
            tracing::warn!(error = %e, %user, %thread_id, "send rows not removed");
        }
    }
    deleted
}

/// POST /v1/users/{user}/trash:empty
pub async fn empty(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    let tids = state.mailbox.trashed_threads(&user).await.map_err(fail)?;
    let mut affected = 0u32;
    for tid in tids {
        if delete_for_good(&state, &user, &tid).await {
            affected += 1;
        }
    }
    Ok(Json(wire::ThreadActionResponse {
        affected,
        new_modseq: 0,
    }))
}

/// One pass of the purge: delete for good up to `batch` conversations
/// trashed at or before `cutoff`. How many it deleted.
pub(crate) async fn purge_once(state: &Arc<CoreRpcState>, cutoff: i64, batch: i64) -> usize {
    let due = match state.mailbox.trashed_before(cutoff, batch).await {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(error = %e, "trash purge: query failed");
            return 0;
        }
    };
    let mut done = 0;
    for (user, tid) in due {
        if delete_for_good(state, &user, &tid).await {
            done += 1;
        } else {
            // a conversation with none of its mail left: nothing to delete
            // but the Trash row, which would otherwise come back every pass
            if let Err(e) = state.mailbox.untrash_thread(&user, &tid).await {
                tracing::warn!(error = %e, %user, %tid, "trash purge: row not removed");
            }
        }
    }
    done
}

/// Purge the Trash for ever, as fastcore does: nothing due costs one
/// indexed query and doubles the wait, up to six hours; a pass that
/// deleted something goes back to the hourly base.
pub(crate) fn spawn_purge(state: Arc<CoreRpcState>) {
    use std::time::Duration;
    const BASE: Duration = Duration::from_secs(3600);
    const MAX: Duration = Duration::from_secs(6 * 3600);
    tokio::spawn(async move {
        let mut wait = BASE;
        loop {
            tokio::time::sleep(wait).await;
            let cutoff = chrono::Utc::now().timestamp() - KEEP_SECS;
            wait = match purge_once(&state, cutoff, 500).await {
                0 => (wait * 2).min(MAX),
                n => {
                    tracing::info!(deleted = n, "trash purge: deleted for good");
                    BASE
                }
            };
        }
    });
}
