//! Reading the maildir directly — the self-heal sweep and the file
//! helpers it shares with the maintenance routes.
//!
//! Split out of `lib.rs` on 2026-08-02, where `healed_from_maildir` alone
//! was 587 lines of a 6,371-line file. It was four phases in a row with
//! nothing but a blank line between them; they are four functions now, and
//! the orchestrator below is the whole shape of the sweep on one screen.

mod files;
mod heal_messages;
mod heal_rows;
mod scan;

pub(crate) use files::*;

use std::sync::Arc;

use crate::FastcoreState;

/// Repair one user's store from what is actually in their maildir.
///
/// Returns whether anything was healed, which is what the caller's backoff
/// reads: a sweep that finds nothing must widen its interval, or it is a
/// busy-wait wearing a repair's clothes.
pub(crate) async fn healed_from_maildir(
    state: &Arc<FastcoreState>,
    user: &str,
    since: i64,
) -> bool {
    let parsed = scan::scan_maildir(user, since);
    if parsed.is_empty() {
        return false;
    }
    let by_root = scan::group_by_thread(state, user, &parsed);

    heal_messages::backfill_uids_once(state, user);
    let (healed_threads, _healed_msgs, diff_healed_threads, _diff_msgs) =
        heal_messages::heal_missing_messages(state, user, &by_root, parsed.len());
    let (rows_healed, created) = heal_rows::heal_membership_rows(state, user, &by_root);

    healed_threads > 0 || diff_healed_threads > 0 || rows_healed > 0 || created > 0
}
