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

use std::path::PathBuf;
use std::sync::Arc;

use crate::FastcoreState;

fn mailbox_dir(user: &str) -> Option<PathBuf> {
    let (local, domain) = user.split_once('@')?;
    let root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    Some(PathBuf::from(root).join(domain).join(local))
}

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
pub(crate) fn apply_to_row(
    state: &Arc<FastcoreState>,
    log: &mailrs_threadstate::ThreadState,
    user: &str,
    thread_id: &str,
) {
    let Some(rec) = log.get(thread_id) else {
        return;
    };
    if let Some(until) = rec.snoozed_until
        && let Err(e) = state.mailbox.set_snoozed(user, thread_id, until)
    {
        tracing::warn!(err = %e, %user, %thread_id, "threadstate: snooze replay failed");
    }
    if let Some(level) = &rec.importance_level {
        let score = rec.importance_score.unwrap_or(0.0);
        if let Err(e) = state
            .mailbox
            .set_thread_importance(user, thread_id, level, score)
        {
            tracing::warn!(err = %e, %user, %thread_id, "threadstate: importance replay failed");
        }
    }
    if let Some(needs) = rec.requires_action
        && let Err(e) = state.mailbox.set_has_action(user, thread_id, needs)
    {
        tracing::warn!(err = %e, %user, %thread_id, "threadstate: action replay failed");
    }
    // `category` is deliberately not replayed here. The bucket a thread
    // sits in is written by `set_bucket`, which also moves it between the
    // declared folder axes; replaying the verdict without that would put
    // the row's category and the list it appears in out of step. The
    // verdict is logged so a rebuild *can* restore it — the restore goes
    // with `maintenance:reindex`, which owns the axes.
}
