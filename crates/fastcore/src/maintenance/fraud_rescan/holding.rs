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
    let row = state
        .mailbox
        .get_thread_for_user(user, tid)
        .map_err(|e| e.to_string())?
        .ok_or("thread membership disappeared")?;
    let stored = state
        .mailbox
        .fraud_verdict(mid)
        .map_err(|e| e.to_string())?;
    // where it was filed on arrival, and why, still stands: the rescan
    // judges the content again, not the filing
    let mut verdict = verdict.clone();
    if let Some(old) = stored
        .as_deref()
        .and_then(|j| serde_json::from_str::<mailrs_inbound::FraudVerdict>(j).ok())
        && verdict.placement.is_empty()
    {
        verdict.placement = old.placement;
    }
    let json = serde_json::to_string(&verdict).map_err(|e| e.to_string())?;
    if stored.as_deref() != Some(&json) {
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
    // The line that says which conversation a run took, so a count of holds
    // can be traced back to the conversations behind it. What the reader
    // has read is left as it is: held mail is out of the badge, and a
    // release must return the conversation as the reader left it.
    if !row.quarantined {
        crate::hold_facts::record_hold(
            state,
            user,
            tid,
            source,
            &verdict.rules_version,
            &crate::hold_facts::verdict_rules(&verdict),
        )?;
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
