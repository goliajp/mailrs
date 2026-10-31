//! The Trash: moving a conversation there and back, emptying it, and the
//! purge that deletes for good what has been there 30 days. Deleting for
//! good is `thread_actions::delete_thread`, the one place that unlinks.

use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, State};
use mailrs_core_api::method::thread as th;

use crate::FastcoreState;
use crate::routes::thread_actions::member_result;

/// How long a conversation stays in the Trash.
pub(crate) const KEEP_SECS: i64 = 30 * 86_400;

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// `POST /v1/users/{user}/threads/{thread_id}/trash`.
pub(crate) async fn trash_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    let ok = state
        .mailbox
        .set_trashed(&user, &thread_id, true, now())
        .unwrap_or(false);
    member_result(ok)
}

/// `POST /v1/users/{user}/threads/{thread_id}/untrash`.
pub(crate) async fn untrash_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    let ok = state
        .mailbox
        .set_trashed(&user, &thread_id, false, now())
        .unwrap_or(false);
    member_result(ok)
}

/// Delete `user`'s `thread_id` for good, as the delete route does.
async fn delete_for_good(state: &Arc<FastcoreState>, user: &str, thread_id: &str) -> bool {
    let res = crate::routes::thread_actions::delete_thread(
        State(state.clone()),
        Path((user.to_string(), thread_id.to_string())),
    )
    .await;
    res.status().is_success()
}

/// `POST /v1/users/{user}/trash:empty`.
pub(crate) async fn empty_trash(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
) -> Json<th::ThreadActionResponse> {
    let tids = state.mailbox.trashed_threads(&user).unwrap_or_default();
    let mut affected = 0u32;
    for tid in tids {
        if delete_for_good(&state, &user, &tid).await {
            affected += 1;
        }
    }
    Json(th::ThreadActionResponse {
        affected,
        new_modseq: 0,
    })
}

/// One pass of the purge: delete for good up to `batch` conversations
/// that went to the Trash before `cutoff`. How many it deleted.
pub(crate) async fn purge_once(state: &Arc<FastcoreState>, cutoff: i64, batch: usize) -> usize {
    let due = match state.mailbox.trashed_before(cutoff, batch) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(error = %e, "trash purge: index unreadable");
            return 0;
        }
    };
    let mut done = 0;
    for (user, tid) in due {
        if delete_for_good(state, &user, &tid).await {
            done += 1;
        }
    }
    done
}

/// Purge the Trash for ever. Nothing due costs an empty index scan and
/// doubles the wait, up to six hours; a pass that deleted something goes
/// back to the hourly base.
pub(crate) fn spawn_purge(state: Arc<FastcoreState>) {
    const BASE: Duration = Duration::from_secs(3600);
    const MAX: Duration = Duration::from_secs(6 * 3600);
    const BATCH: usize = 500;
    tokio::spawn(async move {
        let mut wait = BASE;
        loop {
            tokio::time::sleep(wait).await;
            let done = purge_once(&state, now() - KEEP_SECS, BATCH).await;
            wait = match done {
                0 => (wait * 2).min(MAX),
                n => {
                    tracing::info!(deleted = n, "trash purge: deleted for good");
                    BASE
                }
            };
        }
    });
}

#[cfg(test)]
mod tests;
