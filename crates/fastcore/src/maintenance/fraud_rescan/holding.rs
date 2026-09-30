//! Apply only the changes still needed after a replayed background batch.
use std::sync::Arc;

use crate::FastcoreState;

pub(super) fn apply(
    state: &Arc<FastcoreState>,
    user: &str,
    tid: &str,
    mid: &str,
    verdict: &mailrs_inbound::FraudVerdict,
) -> Result<bool, String> {
    let row = state
        .mailbox
        .get_thread_for_user(user, tid)
        .map_err(|e| e.to_string())?
        .ok_or("thread membership disappeared")?;
    let json = serde_json::to_string(verdict).map_err(|e| e.to_string())?;
    if state
        .mailbox
        .fraud_verdict(mid)
        .map_err(|e| e.to_string())?
        .as_deref()
        != Some(&json)
    {
        state
            .mailbox
            .set_fraud_verdict(mid, &json)
            .map_err(|e| e.to_string())?;
    }
    if !row.quarantined
        && !state
            .mailbox
            .set_quarantined(user, tid, true)
            .map_err(|e| e.to_string())?
    {
        return Err("thread membership disappeared".into());
    }
    let messages = state
        .mailbox
        .user_thread_message_ids(user, tid)
        .map_err(|e| e.to_string())?;
    let mut unread = row.unread_count > 0;
    // Do file renames before advancing the read projection. A rename error
    // leaves this batch retryable, including after a process restart.
    for mid in &messages {
        let facts = state
            .mailbox
            .user_message_facts(user, mid)
            .map_err(|e| e.to_string())?
            .ok_or("message row disappeared")?;
        unread |= facts.flags & 1 == 0;
        crate::maildir_scan::apply_flag_bitmask(user, &facts.blob_ref, facts.flags | 1)
            .map_err(|e| e.to_string())?;
    }
    if unread {
        if !crate::routes::thread_actions::mark_thread_read_everywhere(state, user, tid) {
            return Err("mark read failed".into());
        }
        for mid in &messages {
            if state
                .mailbox
                .user_message_facts(user, mid)
                .map_err(|e| e.to_string())?
                .is_none_or(|facts| facts.flags & 1 == 0)
            {
                return Err("read projection incomplete".into());
            }
        }
    }
    Ok(!row.quarantined)
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
