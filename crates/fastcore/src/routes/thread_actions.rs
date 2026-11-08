//! Per-thread mutations — the verbs the conversation list issues.
//!
//! Every one answers 200 with a `ThreadActionResponse` body, not 204;
//! `local-fastcore-smoke.sh` asked for 204 here and was red from 2026-06
//! to 2026-08-02 without anyone seeing it.

use std::sync::Arc;

use axum::extract::{Path, State};

use crate::*;

/// Mark one thread read in all three places it is recorded: the thread
/// counter, this user's message rows, and the maildir file names.
///
/// The counter alone is what the three read verbs wrote until 2026-08-14,
/// and the shape of that omission: each layer
/// was self-consistent, nothing failed, and mail read in the web stayed
/// bold in every IMAP client. The store may not touch the filesystem, so
/// it reports which files are behind and this brings them in line.
///
/// Returns whatever `mark_seen` returns — whether `user` is in the thread.
pub(crate) fn mark_thread_read_everywhere(
    state: &Arc<FastcoreState>,
    user: &str,
    thread_id: &str,
) -> bool {
    let found = match state.mailbox.mark_seen(user, thread_id) {
        Ok(found) => found,
        Err(e) => {
            tracing::warn!(error = %e, %user, %thread_id, "mark_seen io error — treating as noop");
            false
        }
    };
    match state.mailbox.mark_thread_messages_seen(user, thread_id) {
        Ok(behind) => {
            for (blob_ref, flags) in behind {
                if let Err(e) = crate::maildir_scan::apply_flag_bitmask(user, &blob_ref, flags) {
                    tracing::warn!(error = %e, %user, %blob_ref, "mark read: rename failed");
                }
            }
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
    let member = mark_thread_read_everywhere(&state, &user, &thread_id);
    if was_unread {
        crate::importance::record_engagement(&state, &user, &thread_id, event);
    }
    member_result(member)
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
    if ok {
        crate::keywords::set_on_thread(&state, &user, &thread_id, crate::keywords::PINNED, true);
    }
    member_result(ok)
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
    member_result(ok)
}

pub(crate) async fn unstar_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    member_result(
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
    if ok {
        crate::keywords::set_on_thread(&state, &user, &thread_id, crate::keywords::PINNED, false);
    }
    member_result(ok)
}

pub(crate) async fn archive_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    member_result(set_archived_everywhere(&state, &user, &thread_id, true))
}

/// Archive `thread_id` for `user`, or bring it back to the Inbox: the row,
/// and the keyword bit a rebuilt index finds it by. The web and IMAP both
/// archive through here. `false` when `user` is not in the thread.
pub(crate) fn set_archived_everywhere(
    state: &Arc<FastcoreState>,
    user: &str,
    thread_id: &str,
    archived: bool,
) -> bool {
    // Archiving something still unread is the user dismissing it
    // unseen — the strongest implicit "not worth my attention" signal
    // there is. Read the unread state before the archive write.
    let dismissed_unread = archived
        && state
            .mailbox
            .get_thread_for_user(user, thread_id)
            .ok()
            .flatten()
            .is_some_and(|r| r.unread_count > 0);
    let ok = state
        .mailbox
        .set_archived(user, thread_id, archived)
        .unwrap_or(false);
    // Tier 1: no standard maildir flag means "archived", so it goes in a
    // keyword bit, which is where a rebuilt index can find it again.
    if ok {
        crate::keywords::set_on_thread(state, user, thread_id, crate::keywords::ARCHIVED, archived);
    }
    if ok && dismissed_unread {
        crate::importance::record_engagement(
            state,
            user,
            thread_id,
            mailrs_core_sidestate::families::contacts::Engagement::ArchivedUnread,
        );
    }
    ok
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
        crate::bayes_train::learn_reader_verdict(&state, &user, &thread_id, true);
    }
    member_result(ok)
}

/// v2.4.1 Phase 3 (RFC-B §3.4) — mark a thread as not junk. The
/// webapi layer separately writes to `spam:{user}:whitelist`; this
/// RPC just handles the mailbox side (move the thread + stamp
/// `category = "inbox"`).
pub(crate) async fn mark_not_junk(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    let from_junk = in_junk(&state, &user, &thread_id);
    let ok = state
        .mailbox
        .set_junk(&user, &thread_id, false)
        .unwrap_or(false);
    // v2.8.0: learn this thread as ham. train_thread unlearns any prior
    // spam training on the same thread first (mis-file correction).
    if ok {
        match from_junk {
            true => crate::bayes_train::learn_reader_verdict(&state, &user, &thread_id, false),
            false => crate::bayes_train::train_thread(&state, &user, &thread_id, false),
        }
        // its images were not fetched while it was in Junk
        crate::image_prefetch::queue_thread(&state, &user, &thread_id);
    }
    member_result(ok)
}

/// `thread_id` sits in `user`'s Junk.
fn in_junk(state: &Arc<FastcoreState>, user: &str, thread_id: &str) -> bool {
    state
        .mailbox
        .get_thread_for_user(user, thread_id)
        .ok()
        .flatten()
        .is_some_and(|r| {
            mailrs_mailbox_kevy::scope::bucket_of(&r.category)
                == mailrs_mailbox_kevy::scope::Bucket::Junk
        })
}

/// File `thread_id` into `bucket` for `user`. Out of Junk, whoever put it
/// there, that is the reader correcting a Junk verdict: it is learnt as
/// ham and recorded as a correction, as "not junk" is. `false` when `user`
/// is not in the thread.
fn file_into(
    state: &Arc<FastcoreState>,
    user: &str,
    thread_id: &str,
    bucket: mailrs_mailbox_kevy::keys::Bucket,
) -> bool {
    let from_junk = in_junk(state, user, thread_id);
    let ok = state
        .mailbox
        .set_bucket(user, thread_id, bucket)
        .unwrap_or(false);
    if ok && from_junk {
        crate::bayes_train::learn_reader_verdict(state, user, thread_id, false);
    }
    ok
}

/// v2.9 triage — move a thread into the Notifications bucket and train
/// the triage classifier on this correction.
pub(crate) async fn mark_notification(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    let ok = file_into(
        &state,
        &user,
        &thread_id,
        mailrs_mailbox_kevy::keys::Bucket::Notifications,
    );
    if ok {
        crate::bayes_train::train_triage(&state, &user, &thread_id, "notification");
    }
    member_result(ok)
}

/// v2.9 triage — move a thread into the Promotions bucket and train
/// the triage classifier on this correction.
pub(crate) async fn mark_promotion(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    let ok = file_into(
        &state,
        &user,
        &thread_id,
        mailrs_mailbox_kevy::keys::Bucket::Promotions,
    );
    if ok {
        crate::bayes_train::train_triage(&state, &user, &thread_id, "promotion");
    }
    member_result(ok)
}

/// v2.9 triage — move a thread back into the Inbox bucket and train the
/// triage classifier on this correction.
pub(crate) async fn move_to_inbox(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    let ok = file_into(
        &state,
        &user,
        &thread_id,
        mailrs_mailbox_kevy::keys::Bucket::Inbox,
    );
    if ok {
        crate::bayes_train::train_triage(&state, &user, &thread_id, "inbox");
        // a thread moved out of Junk this way had no images fetched
        crate::image_prefetch::queue_thread(&state, &user, &thread_id);
    }
    member_result(ok)
}

pub(crate) async fn unarchive_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    member_result(set_archived_everywhere(&state, &user, &thread_id, false))
}

pub(crate) async fn delete_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    match delete_for_good(&state, &user, &thread_id) {
        Ok(gone) => member_result(gone.is_some()),
        Err(()) => axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// What deleting a conversation for good took off the disk.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Deleted {
    pub messages: usize,
    pub bytes: u64,
}

/// What a file is removed with: `(state, user, blob ref)`; its size, or
/// `None` when there was no such file.
pub(crate) type RemoveFile = fn(&FastcoreState, &str, &str) -> std::io::Result<Option<u64>>;

/// Delete `user`'s `thread_id` for good, index and files, each file through
/// the one function that removes mail and counts it off the usage.
/// `Ok(None)` when it is not the user's; `Err` when the store could not be
/// written or a file stayed on disk.
pub(crate) fn delete_for_good(
    state: &FastcoreState,
    user: &str,
    thread_id: &str,
) -> Result<Option<Deleted>, ()> {
    delete_removing(
        state,
        user,
        thread_id,
        crate::routes::message_ops::remove_maildir_file,
    )
}

/// [`delete_for_good`], with the files removed by `remove`, so a test can
/// make a removal fail the way no permission can for root.
pub(crate) fn delete_removing(
    state: &FastcoreState,
    user: &str,
    thread_id: &str,
    remove: RemoveFile,
) -> Result<Option<Deleted>, ()> {
    // The kevy side of the delete returns every file the user's copies
    // have had — a Message-ID can sit in Sent and INBOX, or in INBOX once
    // per alias it came through. Any file left on disk is re-imported by
    // self-heal and the "deleted" thread re-appears (prod 2026-07-24, two
    // ghost FYI threads; 2026-10-25, a self-sent message back in minutes
    // from its Sent copy).
    let (existed, files) = match state.mailbox.delete_thread(user, thread_id) {
        Ok(pair) => pair,
        Err(e) => {
            tracing::error!(error = %e, %user, %thread_id, "delete_thread kevy failed");
            return Err(());
        }
    };
    let mut gone = Deleted::default();
    let mut left = 0u32;
    for file in &files {
        match remove(state, user, file) {
            Ok(Some(size)) => {
                gone.messages += 1;
                gone.bytes += size;
            }
            Ok(None) => {}
            Err(e) => {
                tracing::error!(error = %e, %user, %thread_id, %file,
                    "delete_thread: a file stayed on disk; self-heal will file it again");
                left += 1;
            }
        }
    }
    // the conversation comes back with the next self-heal, so the delete
    // did not happen and the caller is told so
    if left > 0 {
        return Err(());
    }
    if !existed {
        return Ok(None);
    }
    if let Err(e) = crate::hold_facts::forget_holds(state, user, thread_id) {
        tracing::error!(error = %e, %user, %thread_id, "delete_thread: the hold facts stayed");
        return Err(());
    }
    tracing::info!(
        %user, %thread_id, files = files.len(), unlinked = gone.messages,
        "delete_thread: cleared thread + unlinked maildir files"
    );
    Ok(Some(gone))
}

/// An action that found no row of the caller's in the thread is a 404:
/// the thread is not theirs to act on, and the answer must not say
/// whether it exists for somebody else.
pub(crate) fn member_result(ok: bool) -> axum::response::Response {
    use axum::response::IntoResponse;
    match ok {
        true => action_result(true),
        false => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}
