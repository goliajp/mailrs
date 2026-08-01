//! Per-thread mutations — the verbs the conversation list issues.
//!
//! Every one answers 200 with a `ThreadActionResponse` body, not 204;
//! `local-fastcore-smoke.sh` asked for 204 here and was red from 2026-06
//! to 2026-08-02 without anyone seeing it.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};

use crate::*;

pub(crate) async fn mark_read(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    // Only a genuine unread -> read transition is an engagement event.
    // `mark_seen` returns whether the hash carried an unread_count
    // field, not whether anything changed, so the unread state has to
    // be read here. Without this a client that re-marks an open thread
    // inflates read_count, and the ranker would later learn from a
    // number that measures UI chatter rather than attention.
    let was_unread = state
        .mailbox
        .get_thread_for_user(&user, &thread_id)
        .ok()
        .flatten()
        .is_some_and(|r| r.unread_count > 0);
    let event = crate::importance::read_event(&state, &thread_id, now_secs());
    if let Err(e) = state.mailbox.mark_seen(&user, &thread_id) {
        tracing::warn!(error = %e, %user, %thread_id, "mark_seen io error — treating as noop");
    }
    if was_unread {
        crate::importance::record_engagement(&state, &user, &thread_id, event);
    }
    action_result(true)
}

/// POST `/v1/users/{user}/conversations:mark-all-read` — sweep every
/// unread thread and flip it to seen in one call. UI's "Mark all as
/// read" button was previously batching only the loaded pagination
/// slice, so users with 99+ unread across pages left the tail
/// untouched.
pub(crate) async fn mark_all_read_route(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
) -> Json<serde_json::Value> {
    let flipped = state.mailbox.mark_all_seen(&user).unwrap_or(0);
    Json(serde_json::json!({ "ok": true, "flipped": flipped }))
}

pub(crate) async fn pin_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    action_result(
        state
            .mailbox
            .set_pinned(&user, &thread_id, true)
            .unwrap_or(false),
    )
}

pub(crate) async fn star_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    let ok = state
        .mailbox
        .set_starred(&user, &thread_id, true)
        .unwrap_or(false);
    if ok {
        crate::importance::record_engagement(
            &state,
            &user,
            &thread_id,
            mailrs_core_sidestate::families::contacts::Engagement::Starred,
        );
    }
    action_result(ok)
}

pub(crate) async fn unstar_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    action_result(
        state
            .mailbox
            .set_starred(&user, &thread_id, false)
            .unwrap_or(false),
    )
}

pub(crate) async fn unpin_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    action_result(
        state
            .mailbox
            .set_pinned(&user, &thread_id, false)
            .unwrap_or(false),
    )
}

pub(crate) async fn archive_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    // Archiving something still unread is the user dismissing it
    // unseen — the strongest implicit "not worth my attention" signal
    // there is. Read the unread state before the archive write.
    let dismissed_unread = state
        .mailbox
        .get_thread_for_user(&user, &thread_id)
        .ok()
        .flatten()
        .is_some_and(|r| r.unread_count > 0);
    let ok = state
        .mailbox
        .set_archived(&user, &thread_id, true)
        .unwrap_or(false);
    if ok && dismissed_unread {
        crate::importance::record_engagement(
            &state,
            &user,
            &thread_id,
            mailrs_core_sidestate::families::contacts::Engagement::ArchivedUnread,
        );
    }
    action_result(ok)
}

/// v2.4.1 Phase 3 (RFC-B §3.4) — mark a thread as junk.
pub(crate) async fn mark_junk(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    let ok = state
        .mailbox
        .set_junk(&user, &thread_id, true)
        .unwrap_or(false);
    if ok {
        crate::importance::record_engagement(
            &state,
            &user,
            &thread_id,
            mailrs_core_sidestate::families::contacts::Engagement::MarkedJunk,
        );
    }
    // v2.8.0: feed the Bayesian corpus off the user's explicit junk
    // verdict (RFC 20260713). Best-effort; never blocks the move.
    if ok {
        crate::bayes_train::train_thread(&state, &user, &thread_id, true);
    }
    action_result(ok)
}

/// v2.4.1 Phase 3 (RFC-B §3.4) — mark a thread as not junk. The
/// webapi layer separately writes to `spam:{user}:whitelist`; this
/// RPC just handles the mailbox side (move the thread + stamp
/// `category = "inbox"`).
pub(crate) async fn mark_not_junk(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    let ok = state
        .mailbox
        .set_junk(&user, &thread_id, false)
        .unwrap_or(false);
    // v2.8.0: learn this thread as ham. train_thread unlearns any prior
    // spam training on the same thread first (mis-file correction).
    if ok {
        crate::bayes_train::train_thread(&state, &user, &thread_id, false);
    }
    action_result(ok)
}

/// v2.9 triage — move a thread into the Notifications bucket and train
/// the triage classifier on this correction.
pub(crate) async fn mark_notification(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    let ok = state
        .mailbox
        .set_bucket(
            &user,
            &thread_id,
            mailrs_mailbox_kevy::keys::Bucket::Notifications,
        )
        .unwrap_or(false);
    if ok {
        crate::bayes_train::train_triage(&state, &user, &thread_id, "notification");
    }
    action_result(ok)
}

/// v2.9 triage — move a thread into the Promotions bucket and train
/// the triage classifier on this correction.
pub(crate) async fn mark_promotion(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    let ok = state
        .mailbox
        .set_bucket(
            &user,
            &thread_id,
            mailrs_mailbox_kevy::keys::Bucket::Promotions,
        )
        .unwrap_or(false);
    if ok {
        crate::bayes_train::train_triage(&state, &user, &thread_id, "promotion");
    }
    action_result(ok)
}

/// v2.9 triage — move a thread back into the Inbox bucket and train the
/// triage classifier on this correction.
pub(crate) async fn move_to_inbox(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    let ok = state
        .mailbox
        .set_bucket(&user, &thread_id, mailrs_mailbox_kevy::keys::Bucket::Inbox)
        .unwrap_or(false);
    if ok {
        crate::bayes_train::train_triage(&state, &user, &thread_id, "inbox");
    }
    action_result(ok)
}

pub(crate) async fn unarchive_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    action_result(
        state
            .mailbox
            .set_archived(&user, &thread_id, false)
            .unwrap_or(false),
    )
}

pub(crate) async fn delete_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    // The kevy side of the delete returns the maildir blob_refs it saw
    // before wiping the message rows. Without unlinking those files
    // here, self-heal's next tick re-imports every one of them and the
    // "deleted" thread re-appears — confirmed on prod 2026-07-24 with
    // two ghost FYI threads that survived multiple UI deletes.
    let (existed, blob_refs) = match state.mailbox.delete_thread(&user, &thread_id) {
        Ok(pair) => pair,
        Err(e) => {
            tracing::warn!(error = %e, %user, %thread_id, "delete_thread kevy failed");
            return action_result(false);
        }
    };
    let mut unlinked = 0u32;
    for blob_ref in &blob_refs {
        if unlink_maildir_file(&user, blob_ref) {
            unlinked += 1;
        }
    }
    if existed {
        tracing::info!(
            %user, %thread_id, messages = blob_refs.len(), unlinked,
            "delete_thread: cleared thread + unlinked maildir files"
        );
    }
    action_result(existed)
}

pub(crate) async fn mark_unread_route(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    if let Err(e) = state.mailbox.mark_unread(&user, &thread_id) {
        tracing::warn!(error = %e, %user, %thread_id, "mark_unread io error — treating as noop");
    }
    action_result(true)
}

pub(crate) async fn snooze_thread_route(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
    Json(req): Json<th::SnoozeRequest>,
) -> axum::response::Response {
    if let Err(e) = state
        .mailbox
        .set_snoozed(&user, &thread_id, req.snoozed_until)
    {
        tracing::warn!(error = %e, %user, %thread_id, "snooze io error");
    }
    axum::http::StatusCode::NO_CONTENT.into_response()
}

pub(crate) async fn unsnooze_thread_route(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    if let Err(e) = state.mailbox.set_snoozed(&user, &thread_id, 0) {
        tracing::warn!(error = %e, %user, %thread_id, "unsnooze io error");
    }
    axum::http::StatusCode::NO_CONTENT.into_response()
}
