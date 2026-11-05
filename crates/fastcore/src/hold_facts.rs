//! Holds and releases of a conversation, as facts.
//!
//! A release is appended to a per-conversation list and never rewritten:
//! who let it go, when, and on whose say. A re-judge consults it so a
//! conversation a person released is not held again for mail that was
//! already there when they released it. A rule change that lets a
//! conversation go is recorded too, under `system`, with the pack that
//! did it, so a pack flipping a conversation back and forth shows.
//!
//! Every hold and every release also writes an audit line. The audit log
//! is capped; the release list is not, because the re-judge reads it.
//!
//! A hold is appended to a list of its own, with the Message-IDs the
//! reader had not read at that moment. A hold does not change what is
//! read, so a release has nothing to put back. Nothing in the product
//! reads the list yet: it is what a later look at "what was unread when
//! this was held" has to go on, the way the release list is for releases.

use mailrs_core_api::method::thread::ReleaseFact;

use crate::FastcoreState;

/// A reader or an administrator released it from the review list.
pub(crate) const PERSON: &str = "person";
/// An administrator put back a whole re-judge batch.
pub(crate) const BATCH: &str = "batch";
/// A rule change: the active pack no longer holds it.
pub(crate) const REJUDGE: &str = "rejudge";

/// The actor of a release no person made.
pub(crate) const SYSTEM: &str = "system";

fn key(user: &str, tid: &str) -> String {
    format!("release:facts:{user}:{tid}")
}

fn hold_key(user: &str, tid: &str) -> String {
    format!("hold:facts:{user}:{tid}")
}

/// One hold of a conversation, as appended. Never rewritten.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct HoldFact {
    /// Unix seconds.
    pub(crate) at: i64,
    /// `ingest`, `rescan`, `recall` or `report`.
    pub(crate) source: String,
    /// The rule pack whose verdict held it; empty for a report.
    pub(crate) rules_version: String,
    /// The reader's messages in the conversation not read when it was held.
    pub(crate) unread_mids: Vec<String>,
}

/// The source of a hold a report made.
pub(crate) const REPORT: &str = "report";

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Append one release and its audit line. `scope` says what the release
/// was part of (`batch=<id>` for a batch put back), empty when it stands
/// alone; it goes into the audit line.
pub(crate) fn record_release(
    state: &FastcoreState,
    user: &str,
    tid: &str,
    source: &str,
    actor: &str,
    rules_version: &str,
    scope: &str,
) -> Result<(), String> {
    let fact = ReleaseFact {
        at: now(),
        source: source.to_string(),
        actor: actor.to_string(),
        rules_version: rules_version.to_string(),
    };
    let json = serde_json::to_vec(&fact).map_err(|e| e.to_string())?;
    state
        .mailbox
        .store_ref()
        .rpush(key(user, tid).as_bytes(), &[json.as_slice()])
        .map_err(|e| e.to_string())?;
    let detail =
        format!("user={user} source={source} actor={actor} rules_version={rules_version} {scope}");
    mailrs_core_sidestate::families::audit::record_by_core(
        state,
        actor,
        "quarantine.release",
        tid,
        detail.trim_end(),
        fact.at,
    );
    Ok(())
}

/// Every release of `tid` for `user`, oldest first.
pub(crate) fn releases(
    state: &FastcoreState,
    user: &str,
    tid: &str,
) -> Result<Vec<ReleaseFact>, String> {
    let rows = state
        .mailbox
        .store_ref()
        .lrange(key(user, tid).as_bytes(), 0, -1)
        .map_err(|e| e.to_string())?;
    rows.iter()
        .map(|r| serde_json::from_slice(r).map_err(|e| e.to_string()))
        .collect()
}

/// When a person last released `tid` for `user`: a reader, or an
/// administrator putting back a batch. A release by a rule change does
/// not count: the rules may take back what the rules let go.
pub(crate) fn person_released_at(
    state: &FastcoreState,
    user: &str,
    tid: &str,
) -> Result<Option<i64>, String> {
    Ok(releases(state, user, tid)?
        .iter()
        .filter(|f| f.source == PERSON || f.source == BATCH)
        .map(|f| f.at)
        .max())
}

/// Whether a re-judge may hold `tid` on a message that arrived at
/// `arrived`: not when a person released the conversation after it came.
/// A message with no arrival time is treated as old, which leaves the
/// conversation where the person put it.
pub(crate) fn may_hold_again(
    state: &FastcoreState,
    user: &str,
    tid: &str,
    arrived: Option<i64>,
) -> Result<bool, String> {
    let Some(released) = person_released_at(state, user, tid)? else {
        return Ok(true);
    };
    Ok(arrived.is_some_and(|a| a > released))
}

/// The audit line and the hold fact for a conversation that was not held
/// and now is.
pub(crate) fn record_hold(
    state: &FastcoreState,
    user: &str,
    tid: &str,
    source: &str,
    rules_version: &str,
    rules: &[String],
) -> Result<(), String> {
    record_hold_fact(state, user, tid, source, rules_version)?;
    let detail = format!(
        "user={user} source={source} rules_version={rules_version} rules={}",
        rules.join(",")
    );
    mailrs_core_sidestate::families::audit::record_by_core(
        state,
        SYSTEM,
        "quarantine.hold",
        tid,
        &detail,
        now(),
    );
    Ok(())
}

/// Append the hold fact alone, for a hold whose audit line is written
/// elsewhere (a report's, by `mailrs_intake::scam_report`).
pub(crate) fn record_hold_fact(
    state: &FastcoreState,
    user: &str,
    tid: &str,
    source: &str,
    rules_version: &str,
) -> Result<(), String> {
    let mut unread_mids = Vec::new();
    for mid in state
        .mailbox
        .user_thread_message_ids(user, tid)
        .map_err(|e| e.to_string())?
    {
        let flags = state
            .mailbox
            .user_message_facts(user, &mid)
            .map_err(|e| e.to_string())?
            .map(|f| f.flags);
        if flags.is_some_and(|f| f & mailrs_core_api::method::message::FLAG_SEEN == 0) {
            unread_mids.push(mid);
        }
    }
    let fact = HoldFact {
        at: now(),
        source: source.to_string(),
        rules_version: rules_version.to_string(),
        unread_mids,
    };
    let json = serde_json::to_vec(&fact).map_err(|e| e.to_string())?;
    state
        .mailbox
        .store_ref()
        .rpush(hold_key(user, tid).as_bytes(), &[json.as_slice()])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Every hold of `tid` for `user`, oldest first.
#[cfg(test)]
pub(crate) fn holds(state: &FastcoreState, user: &str, tid: &str) -> Result<Vec<HoldFact>, String> {
    let rows = state
        .mailbox
        .store_ref()
        .lrange(hold_key(user, tid).as_bytes(), 0, -1)
        .map_err(|e| e.to_string())?;
    rows.iter()
        .map(|r| serde_json::from_slice(r).map_err(|e| e.to_string()))
        .collect()
}

/// Drop the holds of a conversation deleted for good.
pub(crate) fn forget_holds(state: &FastcoreState, user: &str, tid: &str) -> Result<(), String> {
    state
        .mailbox
        .store_ref()
        .del(&[hold_key(user, tid).as_bytes()])
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// The rule ids a stored verdict names.
pub(crate) fn verdict_rules(v: &mailrs_inbound::FraudVerdict) -> Vec<String> {
    v.layers
        .iter()
        .flat_map(|l| l.reasons.iter().map(|r| r.code.clone()))
        .collect()
}

#[cfg(test)]
mod fact_tests;
#[cfg(test)]
mod tests;
