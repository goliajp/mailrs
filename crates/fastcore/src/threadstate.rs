//! The per-thread decisions, written beside the mail.
//!
//! Step 5 of `.claude/rfcs/20260814-the-maildir-is-the-store.md`. A snooze
//! carries a timestamp and a verdict carries a value, so neither fits in a
//! flag or a keyword bit — they go in an append-only log next to the mail,
//! and the membership row stays the index that serves them.
//!
//! **Both directions in the same change**, as with the uidlist and the
//! keywords: the verbs append, and the self-heal replays the log onto the
//! rows a rebuilt index would otherwise be missing. A log nothing reads
//! back is the `one-side-of-the-wire` shape.

use std::sync::Arc;

use crate::FastcoreState;

use crate::maildir_scan::mailbox_dir;

/// Replay a mailbox's log, or an empty state.
pub(crate) fn load(user: &str) -> mailrs_threadstate::ThreadState {
    let Some(dir) = mailbox_dir(user) else {
        return mailrs_threadstate::ThreadState::default();
    };
    mailrs_threadstate::read(&dir).unwrap_or_else(|e| {
        tracing::warn!(err = %e, %user, "threadstate unreadable — treating as none");
        mailrs_threadstate::ThreadState::default()
    })
}

/// Append one decision. Best-effort: a mailbox whose log cannot be written
/// still serves mail, it just has less to rebuild from.
pub(crate) fn record(user: &str, rec: &mailrs_threadstate::Record) {
    let Some(dir) = mailbox_dir(user) else {
        return;
    };
    if let Err(e) = mailrs_threadstate::append(&dir, rec) {
        tracing::warn!(err = %e, %user, tid = %rec.tid, "threadstate append failed");
    }
}

/// A record about `tid`, stamped now.
pub(crate) fn about(thread_id: &str) -> mailrs_threadstate::Record {
    mailrs_threadstate::Record::new(thread_id, crate::now_secs())
}

/// Put the log's decisions back onto one thread's row.
///
/// The reading half. Called by the self-heal, which is the rebuild: on a
/// fresh index the row has defaults and the log has what the reader
/// actually decided.
/// Returns whether anything on the row had to change, so a caller that
/// reports its work reports writes rather than attempts — the
/// `periodic-work-must-converge` shape, and the one the read-state
/// backfill got wrong by counting before writing.
pub(crate) fn apply_to_row(
    state: &Arc<FastcoreState>,
    log: &mailrs_threadstate::ThreadState,
    user: &str,
    thread_id: &str,
) -> bool {
    let Some(rec) = log.get(thread_id) else {
        return false;
    };
    let Ok(Some(row)) = state.mailbox.get_thread_for_user(user, thread_id) else {
        return false;
    };
    let mut changed = false;

    if let Some(until) = rec.snoozed_until
        && row.snoozed_until != until
    {
        match state.mailbox.set_snoozed(user, thread_id, until) {
            Ok(_) => changed = true,
            Err(e) => {
                tracing::warn!(err = %e, %user, %thread_id, "threadstate: snooze replay failed")
            }
        }
    }
    if let Some(level) = &rec.importance_level {
        let score = rec.importance_score.unwrap_or(0.0);
        // The score is a float and the row's copy came back through a
        // string, so compare it loosely enough that a round trip is not a
        // difference — and tightly enough that a real change is one.
        if row.importance_level != *level || (row.importance_score - score).abs() > 1e-6 {
            match state
                .mailbox
                .set_thread_importance(user, thread_id, level, score)
            {
                Ok(()) => changed = true,
                Err(e) => {
                    tracing::warn!(err = %e, %user, %thread_id, "threadstate: importance replay failed")
                }
            }
        }
    }
    if let Some(needs) = rec.requires_action
        && row.has_action != needs
    {
        match state.mailbox.set_has_action(user, thread_id, needs) {
            Ok(_) => changed = true,
            Err(e) => {
                tracing::warn!(err = %e, %user, %thread_id, "threadstate: action replay failed")
            }
        }
    }
    // The classifier's verdict, put back through the one writer that also
    // moves the thread between the declared folder axes. Writing the
    // `category` field alone would leave the row's category and the list
    // it appears in disagreeing, which is why this waited for a caller
    // that owns the axes rather than being done where the record is read.
    if let Some(category) = &rec.category
        && row.category != *category
        && let Some(bucket) = bucket_for(category)
    {
        match state.mailbox.set_bucket(user, thread_id, bucket) {
            Ok(_) => changed = true,
            Err(e) => {
                tracing::warn!(err = %e, %user, %thread_id, "threadstate: category replay failed")
            }
        }
    }
    changed
}

/// The bucket a recorded category belongs to.
///
/// Several categories map onto one bucket and a category this does not
/// know is left alone: moving a thread into a bucket on a guess is worse
/// than leaving it where the reader last saw it.
fn bucket_for(category: &str) -> Option<mailrs_mailbox_kevy::keys::Bucket> {
    use mailrs_mailbox_kevy::keys::Bucket;
    match category {
        "inbox" => Some(Bucket::Inbox),
        "notification" | "notifications" => Some(Bucket::Notifications),
        "promotion" | "promotions" => Some(Bucket::Promotions),
        "spam" | "scam" | "junk" => Some(Bucket::Junk),
        _ => None,
    }
}
