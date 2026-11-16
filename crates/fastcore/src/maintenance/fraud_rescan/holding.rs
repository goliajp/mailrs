//! Apply only the changes still needed after a replayed background batch.
use std::sync::Arc;

use crate::FastcoreState;

pub(crate) fn apply(
    state: &Arc<FastcoreState>,
    user: &str,
    tid: &str,
    mid: &str,
    verdict: &mailrs_inbound::FraudVerdict,
    source: &str,
) -> Result<bool, String> {
    mailrs_intake::rejudge::holding::apply(&**state, user, tid, mid, verdict, source)
}

/// Move a conversation to Junk. `Some(true)` when it was already
/// there, `None` when the write failed (logged).
///
/// **Reads the bucket first.** `set_junk` answers "did the row exist",
/// not "did anything change" — so counting its `true` as a move made
/// `already_junk` a number that could not come out other than zero,
/// and a second run reported moving fifty threads that were already in
/// Junk.
pub(super) fn move_to_junk(state: &FastcoreState, user: &str, tid: &str) -> Option<bool> {
    let was_junk = state
        .mailbox
        .get_thread_for_user(user, tid)
        .ok()
        .flatten()
        .is_some_and(|r| {
            // `bucket_of`, not a literal: the category a Junk row
            // carries is `spam`, and the first version of this
            // compared against "junk" and was therefore never true.
            mailrs_mailbox_kevy::keys::bucket_of(&r.category)
                == mailrs_mailbox_kevy::keys::Bucket::Junk
        });
    match state.mailbox.set_junk(user, tid, true) {
        Ok(_) => Some(was_junk),
        Err(e) => {
            tracing::warn!(err = %e, %user, %tid, "fraud rescan: set_junk failed");
            None
        }
    }
}
