//! One message at a time: which side of the read state is behind, and
//! writing the side that is.

use mailrs_core_api::method::message::FLAG_SEEN;
use mailrs_maildir::Flag;

use super::{Pass, Totals, UserChanges};

/// Bring one message's index bit and file name into agreement. Answers
/// whether the message's index row was written.
pub(super) fn reconcile_message(
    p: &Pass,
    mid: &str,
    thread_level_only: bool,
    t: &mut Totals,
    u: &mut UserChanges,
) -> bool {
    let (state, user) = (p.state, p.user);
    let Ok(Some(facts)) = state.mailbox.user_message_facts(user, mid) else {
        return false;
    };
    if facts.blob_ref.is_empty() {
        t.no_file += 1;
        return false;
    }
    let Some((_, disk_flags)) = p.disk.get(base_id(&facts.blob_ref)) else {
        t.no_file += 1;
        return false;
    };
    t.walked += 1;

    let disk_seen = disk_flags.contains(&Flag::Seen);
    let row_seen = facts.flags & FLAG_SEEN != 0;
    // A thread read at thread level counts as read for every
    // message in it, per the definition above.
    //
    // Only for deciding what to write to the **file**. Comparing
    // it against the disk instead is what made this route unable
    // to converge its own shadow: a message the disk already
    // called read matched the thread's verdict, was counted as
    // agreement, and kept a clear index bit forever — 200 of the
    // 215 `seen_only_on_disk` rows on production, none of them
    // reachable by any number of runs.
    let index_seen = row_seen || thread_level_only;

    if disk_seen && !row_seen {
        return catch_index_up_to_disk(p, mid, t, u);
    }

    if disk_seen == index_seen {
        t.already_agreed += 1;
        return false;
    }

    // Only one case is left: the index says read and the file
    // does not. The disk-ahead direction returned above.
    if p.dry_run {
        if thread_level_only {
            t.thread_level_only_marked += 1;
        } else {
            t.disk_marked_seen += 1;
        }
        return false;
    }

    // The file is behind: write `S`, keeping every flag the
    // bitmask cannot express (`P`), which is why this goes
    // through the shared helper rather than `mark_processed`.
    match crate::maildir_scan::apply_flag_bitmask(user, &facts.blob_ref, facts.flags | FLAG_SEEN) {
        Ok(true) if thread_level_only => {
            t.thread_level_only_marked += 1;
            u.thread += 1;
        }
        Ok(true) => {
            t.disk_marked_seen += 1;
            u.disk += 1;
        }
        Ok(false) => t.already_agreed += 1,
        Err(e) => {
            tracing::warn!(err = %e, %user, %mid, "backfill: rename failed");
            t.errors += 1;
        }
    }
    // The index row's own bit may still be clear on a
    // thread-level-only thread; bring it in line so the
    // shadow converges and the axis is right.
    thread_level_only
        && !row_seen
        && matches!(state.mailbox.mark_user_message_seen(user, mid), Ok(Some(_)))
}

// The disk is ahead of the row, whatever the thread
// counter believes. The file name is the authority.
//
// Counted on the write, not before it: the previous
// version incremented first and wrote through a helper
// that gives up silently when the uid index cannot
// answer — which is every one of these rows, since they
// carry `uid: 0`. It reported 215 repaired on every run
// and repaired none.
fn catch_index_up_to_disk(p: &Pass, mid: &str, t: &mut Totals, u: &mut UserChanges) -> bool {
    let user = p.user;
    if p.dry_run {
        t.index_marked_seen += 1;
        return false;
    }
    match p.state.mailbox.mark_user_message_seen(user, mid) {
        Ok(Some(_)) => {
            t.index_marked_seen += 1;
            u.index += 1;
            true
        }
        Ok(None) => {
            t.already_agreed += 1;
            false
        }
        Err(e) => {
            tracing::warn!(err = %e, %user, %mid, "backfill: row write failed");
            t.errors += 1;
            false
        }
    }
}

/// Same rule as the shadow's: a `blob_ref` loses its subfolder prefix and
/// its flag suffix to give the id the disk map is keyed by.
fn base_id(blob_ref: &str) -> &str {
    let file = blob_ref.rsplit('/').next().unwrap_or(blob_ref);
    file.split(':').next().unwrap_or(file)
}
