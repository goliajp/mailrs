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
