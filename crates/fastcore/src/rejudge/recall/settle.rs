//! A round decides once: what it would hold is gathered while its pages
//! are judged, kept in fastcore's store so a restart loses none of it, and
//! held or set aside when the last page is done.

use std::collections::BTreeMap;

use super::batches::{Entry, entry, put_entry};
use super::*;

/// Conversations one observation may hold without a super administrator's
/// confirmation. `MAILRS_REJUDGE_HOLD_CAP` overrides it.
const HOLD_CAP: usize = 50;

pub(crate) fn hold_cap() -> usize {
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
pub(crate) fn offer(state: &FastcoreState, observation: &str, c: &Candidate) -> Result<(), String> {
    let store = state.mailbox.store_ref();
    let key = candidates_key(observation);
    let field = format!("{}\t{}", c.user, c.tid);
    if store
        .hget(key.as_bytes(), field.as_bytes())
        .map_err(|e| e.to_string())?
        .is_some()
    {
        return Ok(());
    }
    let json = serde_json::to_vec(c).map_err(|e| e.to_string())?;
    store
        .hset(key.as_bytes(), &[(field.as_bytes(), json.as_slice())])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub(super) fn candidates(state: &FastcoreState, observation: &str) -> Vec<Candidate> {
    let mut out: Vec<(Vec<u8>, Candidate)> = state
        .mailbox
        .store_ref()
        .hgetall(candidates_key(observation).as_bytes())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(f, v)| Some((f, serde_json::from_slice(&v).ok()?)))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.into_iter().map(|(_, c)| c).collect()
}

pub(super) fn forget(state: &FastcoreState, observation: &str) {
    let key = candidates_key(observation);
    if let Err(e) = state.mailbox.store_ref().del(&[key.as_bytes()]) {
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
/// more than [`hold_cap`] conversations, or record it as a pending batch
/// that holds nothing. A round already recorded as pending stays so.
pub(crate) fn settle(state: &Arc<FastcoreState>, observation: &str) -> Result<(), String> {
    let gathered = candidates(state, observation);
    if gathered.is_empty() || entry(state, observation).is_some_and(|e| e.pending) {
        return Ok(());
    }
    let mut takeable = Vec::new();
    for c in gathered {
        if may_take(state, &c)? {
            takeable.push(c);
        }
    }
    if takeable.is_empty() {
        forget(state, observation);
        return Ok(());
    }
    let rules = rules_of_all(&takeable);
    let cap = hold_cap();
    let over = takeable.len() > cap;
    put_entry(
        state,
        observation,
        &Entry::new(now(), over, takeable.len() as u64, &rules),
    )?;
    if over {
        tracing::warn!(%observation, would_hold = takeable.len(), cap,
            "rejudge: more to hold than one observation may; held nothing, waiting for a decision");
        if let Some(mut conn) = state.net_conn() {
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
    hold_all(state, observation, &takeable);
    forget(state, observation);
    Ok(())
}

/// Hold each of `cs` that may still be taken, then tell each reader once.
/// Returns how many were held.
pub(super) fn hold_all(state: &Arc<FastcoreState>, observation: &str, cs: &[Candidate]) -> u64 {
    let mut conn = state.net_conn();
    let at = now();
    let mut by_reader: BTreeMap<&str, Vec<&Candidate>> = BTreeMap::new();
    for c in cs {
        match recall(state, c, observation) {
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
