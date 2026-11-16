//! Holds and releases of a conversation, as facts, the same in every
//! store: who released it, when and on whose say, which a re-judge reads so
//! a conversation a person released is not held again for mail that was
//! there when they did; and the audit line beside each.

use mailrs_core_api::method::thread::ReleaseFact;
use mailrs_core_sidestate::families::audit;

use super::store::RejudgeStore;

/// A reader or an administrator released it from the review list.
pub const PERSON: &str = "person";
/// An administrator put back a whole re-judge batch.
pub const BATCH: &str = "batch";
/// A rule change: the active pack no longer holds it.
pub const REJUDGE: &str = "rejudge";
/// The actor of a release no person made.
pub const SYSTEM: &str = "system";
/// A person's report of a scam held it.
pub const REPORT: &str = "report";

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Append one release and its audit line. `scope` says what the release
/// was part of (`batch=<id>` for a batch put back), empty when it stands
/// alone.
pub fn record_release<S: RejudgeStore>(
    store: &S,
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
    store
        .append_release(user, tid, &fact)
        .map_err(|e| e.to_string())?;
    let detail =
        format!("user={user} source={source} actor={actor} rules_version={rules_version} {scope}");
    audit::record_by_core(
        store,
        actor,
        "quarantine.release",
        tid,
        detail.trim_end(),
        fact.at,
    );
    Ok(())
}

/// When a person last released `tid` for `user`: a reader, or an
/// administrator putting back a batch. A release by a rule change does not
/// count: the rules may take back what the rules let go.
pub fn person_released_at<S: RejudgeStore>(
    store: &S,
    user: &str,
    tid: &str,
) -> Result<Option<i64>, String> {
    Ok(store
        .releases(user, tid)
        .map_err(|e| e.to_string())?
        .iter()
        .filter(|f| f.source == PERSON || f.source == BATCH)
        .map(|f| f.at)
        .max())
}

/// Whether a re-judge may hold `tid` on a message that arrived at
/// `arrived`: not when a person released the conversation after it came.
/// A message with no arrival time is treated as old, which leaves the
/// conversation where the person put it.
pub fn may_hold_again<S: RejudgeStore>(
    store: &S,
    user: &str,
    tid: &str,
    arrived: Option<i64>,
) -> Result<bool, String> {
    let Some(released) = person_released_at(store, user, tid)? else {
        return Ok(true);
    };
    Ok(arrived.is_some_and(|a| a > released))
}

/// Whether a person's report holds `tid` now: its newest hold came from a
/// report, so a rule change may not release it.
pub fn held_by_report<S: RejudgeStore>(store: &S, user: &str, tid: &str) -> Result<bool, String> {
    Ok(store
        .last_hold_source(user, tid)
        .map_err(|e| e.to_string())?
        .is_some_and(|s| s == REPORT))
}

/// The audit line for a conversation that was not held and now is.
pub fn audit_hold<S: RejudgeStore>(
    store: &S,
    user: &str,
    tid: &str,
    source: &str,
    rules_version: &str,
    rules: &[String],
) {
    let detail = format!(
        "user={user} source={source} rules_version={rules_version} rules={}",
        rules.join(",")
    );
    audit::record_by_core(store, SYSTEM, "quarantine.hold", tid, &detail, now());
}

/// The rule ids a stored verdict names.
pub fn verdict_rules(v: &mailrs_inbound::FraudVerdict) -> Vec<String> {
    v.layers
        .iter()
        .flat_map(|l| l.reasons.iter().map(|r| r.code.clone()))
        .collect()
}
