//! The maildir's UID map, and the two directions it is used in.
//!
//! Step 3 of `.claude/rfcs/20260814-the-maildir-is-the-store.md`. A UID is a
//! promise to an IMAP client and cannot be recomputed, so it is a fact and
//! belongs beside the mail; the index's copy is a cache of it. What that
//! buys is in `.claude/two-lane-known-diff.txt` §7, which currently records
//! the opposite as accepted: switch lanes and every client resyncs.
//!
//! **Both directions ship together, deliberately.** A file written and
//! never read is the `one-side-of-the-wire` shape this repo has now been
//! bitten by five times in one day — it looks finished from both ends and
//! does nothing. So:
//!
//! - the ingest path **writes** a record when it allocates a UID;
//! - the self-heal — which is the rebuild — **reads** the file first and
//!   adopts what it finds, allocating only for a message the file has never
//!   seen.
//!
//! Arbitration is the RFC's: tier 1 wins. If the file names a UID for a
//! message and the index disagrees, the index is wrong by definition, and
//! `register_uid` corrects it.

use std::sync::Arc;

use crate::FastcoreState;

use crate::maildir_scan::mailbox_dir;

/// Read a user's uidlist once, for a sweep that is about to ask it about
/// many messages.
///
/// Per-message reads would reparse the whole file per message — quadratic
/// on a thirty-thousand-message mailbox, which is the size of the one this
/// was written against.
pub(crate) fn load(user: &str) -> Option<mailrs_uidlist::UidList> {
    let dir = mailbox_dir(user)?;
    match mailrs_uidlist::read(&dir) {
        Ok(list) => list,
        Err(e) => {
            // Unparseable rather than absent. Allocating fresh UIDs over
            // the top of a file that still promises the old ones is the one
            // outcome worth refusing, so this returns None and lets the
            // caller allocate — the file is left alone for a person to look
            // at, and `record` will keep appending to it.
            tracing::warn!(err = %e, %user, "uidlist unreadable — not adopting from it");
            None
        }
    }
}

/// Append `uid -> blob_ref`. Best-effort: a mailbox whose uidlist cannot be
/// written still serves mail, it just has nothing to rebuild from.
pub(crate) fn record(user: &str, uid: u32, blob_ref: &str) {
    if uid == 0 || blob_ref.is_empty() {
        return;
    }
    let Some(dir) = mailbox_dir(user) else {
        return;
    };
    if let Err(e) = mailrs_uidlist::append(&dir, uid, blob_ref) {
        tracing::warn!(err = %e, %user, uid, %blob_ref, "uidlist append failed");
    }
}

/// The UID for one message, preferring the file.
///
/// `list` is what [`load`] returned at the top of the sweep. When it names
/// this file, that UID is adopted into the index and returned; otherwise a
/// fresh one is allocated and appended, so the next rebuild finds it.
pub(crate) fn uid_for(
    state: &Arc<FastcoreState>,
    list: Option<&mailrs_uidlist::UidList>,
    user: &str,
    message_id: &str,
    blob_ref: &str,
) -> u32 {
    if let Some(uid) = list.and_then(|l| l.uid_of(blob_ref)) {
        // Tier 1 wins. `register_uid` is idempotent and raises the
        // allocation counter, so a rebuilt index cannot later hand this
        // number to a different message.
        if let Err(e) = state.mailbox.register_uid(user, uid, message_id) {
            tracing::warn!(err = %e, %user, uid, "uidlist: adopting into the index failed");
        }
        return uid;
    }
    let uid = state.mailbox.allocate_uid(user, message_id).unwrap_or(0);
    record(user, uid, blob_ref);
    uid
}

/// Append many records in one open.
///
/// The backfill has thirty thousand of them per mailbox and [`record`]
/// opens the file per call; this is the same write, once.
pub(crate) fn extend(user: &str, records: &[(u32, String)]) -> std::io::Result<()> {
    let Some(dir) = mailbox_dir(user) else {
        return Ok(());
    };
    mailrs_uidlist::append_many(
        &dir,
        &records
            .iter()
            .map(|(uid, name)| (*uid, name.as_str()))
            .collect::<Vec<_>>(),
    )
}

/// Rewrite a user's uidlist with one record per message, in UID order.
///
/// Housekeeping for the append-only write path, and the repair for a
/// mailbox whose file was never written: the entries come from the file
/// itself where it has them, so a compaction can never invent or retire a
/// promise. Returns `(before, after)` record counts.
pub(crate) fn compact(user: &str) -> std::io::Result<(usize, usize)> {
    let Some(dir) = mailbox_dir(user) else {
        return Ok((0, 0));
    };
    let Some(list) = mailrs_uidlist::read(&dir)? else {
        return Ok((0, 0));
    };
    let before = list.entries.len();
    let compacted = list.compacted();
    let after = compacted.entries.len();
    if after != before {
        mailrs_uidlist::rewrite(&dir, &compacted)?;
    }
    Ok((before, after))
}
