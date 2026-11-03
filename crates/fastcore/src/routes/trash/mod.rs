//! The Trash: moving a conversation there and back, emptying it, and the
//! purge that deletes for good what has been there 30 days. Deleting for
//! good is `thread_actions::delete_for_good`.

use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mailrs_core_api::method::thread as th;

use mailrs_core_sidestate::families::audit::Purged;

use crate::FastcoreState;
use crate::routes::thread_actions::{Deleted, member_result};

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
    match state.mailbox.set_trashed(&user, &thread_id, true, now()) {
        Ok(ok) => member_result(ok),
        Err(e) => store_failed(e, &user, &thread_id),
    }
}

/// `POST /v1/users/{user}/threads/{thread_id}/untrash`.
pub(crate) async fn untrash_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    match state.mailbox.set_trashed(&user, &thread_id, false, now()) {
        Ok(ok) => member_result(ok),
        Err(e) => store_failed(e, &user, &thread_id),
    }
}

/// Delete `user`'s `thread_id` for good, as the delete route does. What it
/// took; `Ok(None)` when it is not the user's; `Err` when it was not
/// deleted.
fn delete_for_good(
    state: &FastcoreState,
    user: &str,
    thread_id: &str,
) -> Result<Option<Deleted>, ()> {
    let Some(gone) = crate::routes::thread_actions::delete_for_good(state, user, thread_id)? else {
        return Ok(None);
    };
    // the Send rows for the mail in it go too, or the Send list keeps
    // showing them with nothing behind
    if let Some(mut c) = state.net_conn()
        && let Err(e) =
            mailrs_core_sidestate::families::send::delete_sends_in_thread(&mut c, user, thread_id)
    {
        tracing::warn!(error = %e, %user, %thread_id, "send rows not removed");
    }
    Ok(Some(gone))
}

fn store_failed(e: std::io::Error, user: &str, thread_id: &str) -> axum::response::Response {
    use axum::response::IntoResponse;
    tracing::error!(error = %e, %user, %thread_id, "trash: the store failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// `POST /v1/users/{user}/trash:empty`: each conversation deleted, sorted,
/// so the caller can write one audit row for each. A Trash that cannot be
/// read, or a conversation in it that could not be deleted, is a 500, not
/// an empty Trash.
pub(crate) async fn empty_trash(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
) -> Result<Json<th::EmptyTrashResponse>, StatusCode> {
    let mut tids = state.mailbox.trashed_threads(&user).map_err(|e| {
        tracing::error!(error = %e, %user, "empty trash: the Trash is unreadable");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    tids.sort();
    let mut deleted = Vec::new();
    let mut failed = 0u32;
    for tid in tids {
        match delete_for_good(&state, &user, &tid) {
            Ok(Some(gone)) => deleted.push(th::DeletedThreadWire {
                thread_id: tid,
                messages: u32::try_from(gone.messages).unwrap_or(u32::MAX),
            }),
            Ok(None) => {}
            Err(()) => failed += 1,
        }
    }
    if failed > 0 {
        tracing::error!(%user, deleted = deleted.len(), failed,
            "empty trash: some conversations were not deleted");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    Ok(Json(th::EmptyTrashResponse {
        affected: u32::try_from(deleted.len()).unwrap_or(u32::MAX),
        new_modseq: 0,
        deleted,
    }))
}

/// One pass of the purge: delete for good up to `batch` conversations
/// that went to the Trash before `cutoff`, and write one audit row per
/// account it deleted from. How many it deleted.
pub(crate) async fn purge_once(state: &Arc<FastcoreState>, cutoff: i64, batch: usize) -> usize {
    let due = match state.mailbox.trashed_before(cutoff, batch) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(error = %e, "trash purge: index unreadable");
            return 0;
        }
    };
    let mut purged = Purged::default();
    for (user, tid) in due {
        if let Ok(Some(gone)) = delete_for_good(state, &user, &tid) {
            purged.add(&user, gone.messages, gone.bytes);
        }
    }
    let mut net = state.net_conn();
    purged.record(net.as_mut(), now())
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
