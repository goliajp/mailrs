//! A reader's acts on a conversation: read, unread, star, pin, archive,
//! snooze. The answer is the same on every store: 200 with one affected
//! conversation for a member, 404 for anyone else; a snooze answers 204.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_core_api::method::thread as wire;
use mailrs_core_sidestate::NetKevy;
use mailrs_core_sidestate::families::correspondent::{self, Before, ReaderAct};

use crate::store::{ReaderView, StoreResult, ThreadStore};

type Thread = Path<(String, String)>;

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The answer to an act on a conversation `user` is or is not in.
fn acted(member: StoreResult<bool>, what: &str, user: &str, thread_id: &str) -> Response {
    match member {
        Ok(true) => Json(wire::ThreadActionResponse {
            affected: 1,
            new_modseq: 0,
        })
        .into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::warn!(error = %e, %user, %thread_id, what, "thread act failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Credit what `act` says, judged against the conversation as it was
/// before, to its correspondent, in the network kevy. Best-effort: an act
/// never fails because a counter could not be written.
async fn credit<S: NetKevy>(
    state: &Arc<S>,
    user: &str,
    act: ReaderAct,
    before: Option<ReaderView>,
) {
    let Some(view) = before else {
        return;
    };
    let seen = Before {
        unread: view.unread,
        latest: view.latest,
    };
    let Some(event) = correspondent::engagement_of(act, seen) else {
        return;
    };
    let (state, user) = (state.clone(), user.to_string());
    let written = tokio::task::spawn_blocking(move || {
        let mut conn = state
            .net_conn()
            .ok_or_else(|| std::io::Error::other("no network kevy"))?;
        correspondent::record_thread_engagement(&mut conn, &user, &view.participants, event)
    })
    .await;
    if let Ok(Err(e)) = written {
        tracing::warn!(error = %e, "engagement not recorded");
    }
}

/// The view before an act, for crediting it; one that cannot be read
/// costs the credit, never the act.
async fn before<S: ThreadStore>(state: &S, user: &str, thread_id: &str) -> Option<ReaderView> {
    state
        .reader_view(user, thread_id)
        .await
        .inspect_err(|e| tracing::warn!(error = %e, %user, %thread_id, "reader view unreadable"))
        .ok()
        .flatten()
}

/// POST /v1/users/{user}/threads/{thread_id}/read
pub async fn mark_read<S: ThreadStore + NetKevy>(
    State(state): State<Arc<S>>,
    Path((user, thread_id)): Thread,
) -> Response {
    let was = before(&*state, &user, &thread_id).await;
    let member = state.mark_read(&user, &thread_id).await;
    if matches!(member, Ok(true)) {
        credit(&state, &user, ReaderAct::Read { now: now_secs() }, was).await;
    }
    acted(member, "mark_read", &user, &thread_id)
}

/// POST /v1/users/{user}/threads/{thread_id}/unread
pub async fn mark_unread<S: ThreadStore>(
    State(state): State<Arc<S>>,
    Path((user, thread_id)): Thread,
) -> Response {
    let member = state.mark_unread(&user, &thread_id).await;
    acted(member, "mark_unread", &user, &thread_id)
}

/// POST /v1/users/{user}/threads/{thread_id}/star
pub async fn star<S: ThreadStore + NetKevy>(
    State(state): State<Arc<S>>,
    Path((user, thread_id)): Thread,
) -> Response {
    let was = before(&*state, &user, &thread_id).await;
    let member = state.set_starred(&user, &thread_id, true).await;
    if matches!(member, Ok(true)) {
        credit(&state, &user, ReaderAct::Starred, was).await;
    }
    acted(member, "star", &user, &thread_id)
}

/// POST /v1/users/{user}/threads/{thread_id}/unstar
pub async fn unstar<S: ThreadStore>(
    State(state): State<Arc<S>>,
    Path((user, thread_id)): Thread,
) -> Response {
    let member = state.set_starred(&user, &thread_id, false).await;
    acted(member, "unstar", &user, &thread_id)
}

/// POST /v1/users/{user}/threads/{thread_id}/pin
pub async fn pin<S: ThreadStore>(
    State(state): State<Arc<S>>,
    Path((user, thread_id)): Thread,
) -> Response {
    let member = state.set_pinned(&user, &thread_id, true).await;
    acted(member, "pin", &user, &thread_id)
}

/// POST /v1/users/{user}/threads/{thread_id}/unpin
pub async fn unpin<S: ThreadStore>(
    State(state): State<Arc<S>>,
    Path((user, thread_id)): Thread,
) -> Response {
    let member = state.set_pinned(&user, &thread_id, false).await;
    acted(member, "unpin", &user, &thread_id)
}

/// POST /v1/users/{user}/threads/{thread_id}/archive
pub async fn archive<S: ThreadStore + NetKevy>(
    State(state): State<Arc<S>>,
    Path((user, thread_id)): Thread,
) -> Response {
    let was = before(&*state, &user, &thread_id).await;
    let member = state.set_archived(&user, &thread_id, true).await;
    if matches!(member, Ok(true)) {
        credit(&state, &user, ReaderAct::Archived, was).await;
    }
    acted(member, "archive", &user, &thread_id)
}

/// POST /v1/users/{user}/threads/{thread_id}/unarchive
pub async fn unarchive<S: ThreadStore>(
    State(state): State<Arc<S>>,
    Path((user, thread_id)): Thread,
) -> Response {
    let member = state.set_archived(&user, &thread_id, false).await;
    acted(member, "unarchive", &user, &thread_id)
}

/// PUT /v1/users/{user}/threads/{thread_id}/snooze
pub async fn snooze<S: ThreadStore>(
    State(state): State<Arc<S>>,
    Path((user, thread_id)): Thread,
    Json(req): Json<wire::SnoozeRequest>,
) -> Response {
    snoozed(&*state, &user, &thread_id, req.snoozed_until).await
}

/// DELETE /v1/users/{user}/threads/{thread_id}/snooze
pub async fn unsnooze<S: ThreadStore>(
    State(state): State<Arc<S>>,
    Path((user, thread_id)): Thread,
) -> Response {
    snoozed(&*state, &user, &thread_id, 0).await
}

async fn snoozed<S: ThreadStore>(state: &S, user: &str, thread_id: &str, until: i64) -> Response {
    match state.set_snoozed(user, thread_id, until).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::warn!(error = %e, %user, %thread_id, "snooze failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
