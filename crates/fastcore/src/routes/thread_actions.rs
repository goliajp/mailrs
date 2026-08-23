//! Per-thread mutations — the verbs the conversation list issues.
//!
//! Every one answers 200 with a `ThreadActionResponse` body, not 204;
//! `local-fastcore-smoke.sh` asked for 204 here and was red from 2026-06
//! to 2026-08-02 without anyone seeing it.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};

use crate::*;

/// Mark one thread read in all three places it is recorded: the thread
/// counter, this user's message rows, and the maildir file names.
///
/// The counter alone is what the three read verbs wrote until 2026-08-14,
/// and the shape of that omission is `one-side-of-the-wire`: each layer
/// was self-consistent, nothing failed, and mail read in the web stayed
/// bold in every IMAP client. The store may not touch the filesystem, so
/// it reports which files are behind and this brings them in line.
///
/// Returns whatever `mark_seen` returns — whether the thread row existed.
fn mark_thread_read_everywhere(state: &Arc<FastcoreState>, user: &str, thread_id: &str) -> bool {
    let found = match state.mailbox.mark_seen(user, thread_id) {
        Ok(found) => found,
        Err(e) => {
            tracing::warn!(error = %e, %user, %thread_id, "mark_seen io error — treating as noop");
            false
        }
    };
    match state.mailbox.mark_thread_messages_seen(user, thread_id) {
        Ok(behind) => {
            let mut refs = Vec::new();
            for (blob_ref, flags) in behind {
                if let Err(e) = crate::maildir_scan::apply_flag_bitmask(user, &blob_ref, flags) {
                    tracing::warn!(error = %e, %user, %blob_ref, "mark read: rename failed");
                }
                refs.push(blob_ref);
            }
            // And the fourth place: the server this came from, if it
            // came from one. Left as a note rather than done here —
            // this call answers somebody pressing a key and must not
            // wait on a round trip to a mailbox that may be asleep.
            crate::external_writeback::note_read(state, &refs);
        }
        Err(e) => tracing::warn!(error = %e, %user, %thread_id, "mark read: row sync failed"),
    }
    found
}

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
    mark_thread_read_everywhere(&state, &user, &thread_id);
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
    // Looped here rather than through `mark_all_seen`, which is the same
    // loop inside the store — and the store cannot rename a file.
    let unread = state
        .mailbox
        .list_thread_ids_by_flag_via_table(&user, "unread", 100_000, 0, None)
        .unwrap_or_default();
    let mut flipped = 0u32;
    for tid in &unread {
        if mark_thread_read_everywhere(&state, &user, tid) {
            flipped += 1;
        }
    }
    Json(serde_json::json!({ "ok": true, "flipped": flipped }))
}

/// Mark read everything **this list** is showing.
///
/// The mailbox-wide version above is the wrong answer when somebody is
/// looking at one folder: "mark all as read" from inside Notifications
/// should not silence the inbox. This takes the same
/// `ConversationFilter` the list read takes and marks exactly what
/// that query returns — one reading of the filter, shared with
/// `list_conversations`, so the set marked and the set on screen
/// cannot be different sets.
///
/// Not paged: the point is the whole list, including the rows the
/// client has not scrolled to. A client marking only its loaded page
/// would look finished and do a fraction, which is the failure this
/// route exists to avoid.
pub(crate) async fn mark_list_read_route(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
    Json(req): Json<conv::ListConversationsRequest>,
) -> Json<serde_json::Value> {
    let filter = crate::routes::reads::threads_filter(&req.filter);
    // Whatever the list would show, ignoring the page size the caller
    // sent — 100_000 is the same ceiling `mark_all_seen` uses.
    let (rows, _total) = match state
        .mailbox
        .list_threads_by_activity(&user, &filter, 0, 100_000)
    {
        Ok(page) => page,
        Err(e) => {
            tracing::warn!(%user, error = %e, "mark-list-read: list failed");
            return Json(serde_json::json!({ "ok": false, "flipped": 0 }));
        }
    };
    let mut flipped = 0u32;
    for row in &rows {
        // Only the ones that were unread: `mark_seen` answers whether
        // the row existed, not whether anything changed, and a count
        // that includes already-read threads would report work it did
        // not do.
        if row.unread_count > 0 && mark_thread_read_everywhere(&state, &user, &row.thread_id) {
            flipped += 1;
        }
    }
    Json(serde_json::json!({ "ok": true, "flipped": flipped }))
}

pub(crate) async fn pin_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    let ok = state
        .mailbox
        .set_pinned(&user, &thread_id, true)
        .unwrap_or(false);
    // Tier 1: a person's decision, written where a rebuilt index cannot
    // lose it. The row above is the index the list reads.
    crate::keywords::set_on_thread(&state, &user, &thread_id, crate::keywords::PINNED, true);
    action_result(ok)
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
    let ok = state
        .mailbox
        .set_pinned(&user, &thread_id, false)
        .unwrap_or(false);
    crate::keywords::set_on_thread(&state, &user, &thread_id, crate::keywords::PINNED, false);
    action_result(ok)
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
    // Tier 1: no standard maildir flag means "archived", so it goes in a
    // keyword bit, which is where a rebuilt index can find it again.
    crate::keywords::set_on_thread(&state, &user, &thread_id, crate::keywords::ARCHIVED, true);
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
    let ok = state
        .mailbox
        .set_archived(&user, &thread_id, false)
        .unwrap_or(false);
    crate::keywords::set_on_thread(&state, &user, &thread_id, crate::keywords::ARCHIVED, false);
    action_result(ok)
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
    // Tier 1: a snooze carries a timestamp, so no flag or keyword bit can
    // hold it — it goes in the log beside the mail, where a rebuilt index
    // can read it back.
    let mut rec = crate::threadstate::about(&thread_id);
    rec.snoozed_until = Some(req.snoozed_until);
    crate::threadstate::record(&user, &rec);
    axum::http::StatusCode::NO_CONTENT.into_response()
}

pub(crate) async fn unsnooze_thread_route(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    if let Err(e) = state.mailbox.set_snoozed(&user, &thread_id, 0) {
        tracing::warn!(error = %e, %user, %thread_id, "unsnooze io error");
    }
    // Zero is a value here, not an absence: it un-snoozes, and a replay
    // that could not tell it from "nothing said" would put the thread
    // away again.
    let mut rec = crate::threadstate::about(&thread_id);
    rec.snoozed_until = Some(0);
    crate::threadstate::record(&user, &rec);
    axum::http::StatusCode::NO_CONTENT.into_response()
}
