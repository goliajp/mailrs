//! A round decides once: what it would hold is gathered while its pages
//! are judged, kept in the store so a restart loses none of it, and held
//! or set aside when the last page is done.

use std::collections::BTreeMap;

use super::batches::{Entry, entry, put_entry};
use super::*;

/// Conversations one observation may hold without a super administrator's
/// confirmation. `MAILRS_REJUDGE_HOLD_CAP` overrides it.
const HOLD_CAP: usize = 50;

pub fn hold_cap() -> usize {
    std::env::var("MAILRS_REJUDGE_HOLD_CAP")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(HOLD_CAP)
}

/// What `observation`'s round would hold: `user\ttid` → [`Candidate`]. A
/// pending batch keeps it until it is confirmed or discarded.
fn candidates_key(observation: &str) -> String {
    format!("rejudge:candidates:{observation}")
}

/// Keep `c` for `observation`'s decision. The first message found for a
/// conversation stands for it.
pub fn offer<S: RejudgeStore>(store: &S, observation: &str, c: &Candidate) -> Result<(), String> {
    let key = candidates_key(observation);
    let field = format!("{}\t{}", c.user, c.tid);
    if store
        .state_hget(&key, &field)
        .map_err(|e| e.to_string())?
        .is_some()
    {
        return Ok(());
    }
    let json = serde_json::to_vec(c).map_err(|e| e.to_string())?;
    store
        .state_hset(&key, &field, &json)
        .map_err(|e| e.to_string())
}

pub(super) fn candidates<S: RejudgeStore>(store: &S, observation: &str) -> Vec<Candidate> {
    let mut out: Vec<(String, Candidate)> = store
        .state_hgetall(&candidates_key(observation))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(f, v)| Some((f, serde_json::from_slice(&v).ok()?)))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.into_iter().map(|(_, c)| c).collect()
}

pub(super) fn forget<S: RejudgeStore>(store: &S, observation: &str) {
    if let Err(e) = store.state_del(&candidates_key(observation)) {
        tracing::warn!(error = %e, %observation, "rejudge: the gathered conversations were not cleared");
    }
}

pub(super) fn rules_of_all(cs: &[Candidate]) -> Vec<String> {
    let mut rules: Vec<String> = cs.iter().flat_map(|c| c.rules.clone()).collect();
    rules.sort();
    rules.dedup();
    rules
}

/// Decide `observation`'s round: hold what it gathered when that is no
/// more than `MAILRS_REJUDGE_HOLD_CAP` (50) conversations, or record it as a pending batch
/// that holds nothing. A round already recorded as pending stays so.
pub fn settle<S: RejudgeStore>(store: &S, observation: &str) -> Result<(), String> {
    let gathered = candidates(store, observation);
    if gathered.is_empty() || entry(store, observation).is_some_and(|e| e.pending) {
        return Ok(());
    }
    let mut takeable = Vec::new();
    for c in gathered {
        if may_take(store, &c)? {
            takeable.push(c);
        }
    }
    if takeable.is_empty() {
        forget(store, observation);
        return Ok(());
    }
    let rules = rules_of_all(&takeable);
    let cap = hold_cap();
    let over = takeable.len() > cap;
    put_entry(
        store,
        observation,
        &Entry::new(now(), over, takeable.len() as u64, &rules),
    )?;
    if over {
        tracing::warn!(%observation, would_hold = takeable.len(), cap,
            "rejudge: more to hold than one observation may; held nothing, waiting for a decision");
        if let Some(mut conn) = store.net_conn() {
            let detail = format!(
                "batch={observation} would_hold={} cap={cap} rules={}",
                takeable.len(),
                rules.join(",")
            );
            if let Err(e) = mailrs_core_sidestate::families::audit::append(
                &mut conn,
                "rejudge",
                "rejudge.batch_pending",
                observation,
                &detail,
                now(),
            ) {
                tracing::warn!(error = %e, %observation, "rejudge: the pending batch was not audited");
            }
        }
        return Ok(());
    }
    hold_all(store, observation, &takeable);
    forget(store, observation);
    Ok(())
}

/// Hold each of `cs` that may still be taken, then tell each reader once.
/// Returns how many were held.
pub(super) fn hold_all<S: RejudgeStore>(store: &S, observation: &str, cs: &[Candidate]) -> u64 {
    let mut conn = store.net_conn();
    let at = now();
    let mut by_reader: BTreeMap<&str, Vec<&Candidate>> = BTreeMap::new();
    for c in cs {
        match recall(store, c, observation) {
            Ok(true) => {
                crate::rejudge::Counts::add(&crate::rejudge::COUNTS.acted, 1);
                tracing::info!(event = "rejudge_acted", user = %c.user, tid = %c.tid,
                    %observation, action = "hold", rules = ?c.rules, "rejudge: acted");
                if let Some(conn) = conn.as_mut() {
                    tell::conversation(conn, c, observation, at);
                }
                by_reader.entry(&c.user).or_default().push(c);
            }
            Ok(false) => {}
            Err(e) => {
                tracing::warn!(error = %e, user = %c.user, tid = %c.tid, "rejudge: acting failed")
            }
        }
    }
    let held = by_reader.values().map(|v| v.len() as u64).sum();
    let Some(conn) = conn.as_mut() else {
        if held > 0 {
            tracing::warn!(%observation, "rejudge: no shared store; the readers were not told");
        }
        return held;
    };
    for (user, taken) in by_reader {
        tell::reader(conn, user, observation, &taken, at);
    }
    held
}
