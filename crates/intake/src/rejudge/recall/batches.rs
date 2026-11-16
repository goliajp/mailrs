//! The batches re-judge rounds took back, for the administrator to list and
//! put back whole, and the pending ones a super administrator confirms or
//! discards.

use super::settle::{candidates, forget, hold_all};
use super::*;

/// A batch as stored under [`BATCHES`]. Batches written before rounds
/// could wait (and those reports write) carry only `at` and `released_at`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct Entry {
    pub(super) at: i64,
    pub(super) released_at: i64,
    #[serde(default)]
    pub(super) pending: bool,
    #[serde(default)]
    pub(super) would_hold: u64,
    #[serde(default)]
    pub(super) rules: Vec<String>,
    #[serde(default)]
    pub(super) discarded_at: i64,
    #[serde(default)]
    pub(super) confirmed_at: i64,
}

impl Entry {
    pub(super) fn new(at: i64, pending: bool, would_hold: u64, rules: &[String]) -> Self {
        Entry {
            at,
            released_at: 0,
            pending,
            would_hold,
            rules: rules.to_vec(),
            discarded_at: 0,
            confirmed_at: 0,
        }
    }
}

pub(super) fn entry<S: RejudgeStore>(store: &S, observation: &str) -> Option<Entry> {
    let raw = store.state_hget(BATCHES, observation).ok().flatten()?;
    serde_json::from_slice(&raw).ok()
}

pub(super) fn put_entry<S: RejudgeStore>(
    store: &S,
    observation: &str,
    e: &Entry,
) -> Result<(), String> {
    let json = serde_json::to_vec(e).map_err(|e| e.to_string())?;
    store
        .state_hset(BATCHES, observation, &json)
        .map_err(|e| e.to_string())
}

/// One batch, for the administrator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    pub observation: String,
    pub at: i64,
    pub conversations: u64,
    pub released_at: i64,
    pub pending: bool,
    pub would_hold: u64,
    pub rules: Vec<String>,
    pub discarded_at: i64,
    pub confirmed_at: i64,
}

/// Why a decision on a batch was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// No such batch.
    Missing,
    /// The batch is not in the state the decision is for: a pending or a
    /// discarded one holds nothing to put back, a decided one cannot be
    /// decided again.
    Conflict,
}

/// Every batch, newest first.
pub fn batches<S: RejudgeStore>(store: &S) -> Vec<Batch> {
    let mut out: Vec<Batch> = store
        .state_hgetall(BATCHES)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(observation, raw)| {
            let e: Entry = serde_json::from_slice(&raw).ok()?;
            let conversations = store
                .state_smembers(&batch_key(&observation))
                .map(|m| m.len() as u64)
                .unwrap_or(0);
            Some(Batch {
                observation,
                at: e.at,
                conversations,
                released_at: e.released_at,
                pending: e.pending,
                would_hold: e.would_hold,
                rules: e.rules,
                discarded_at: e.discarded_at,
                confirmed_at: e.confirmed_at,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        b.at.cmp(&a.at)
            .then_with(|| a.observation.cmp(&b.observation))
    });
    out
}

fn audit<S: RejudgeStore>(store: &S, actor: &str, action: &str, observation: &str, detail: &str) {
    let Some(mut c) = store.net_conn() else {
        tracing::warn!(%action, %observation, "rejudge: no shared store; the batch was not audited");
        return;
    };
    if let Err(error) = mailrs_core_sidestate::families::audit::append(
        &mut c,
        actor,
        action,
        observation,
        detail,
        now(),
    ) {
        tracing::warn!(%error, %action, %observation, "rejudge: the batch audit line was not written");
    }
}

fn pending_entry<S: RejudgeStore>(store: &S, observation: &str) -> Result<Entry, Refused> {
    let e = entry(store, observation).ok_or(Refused::Missing)?;
    if !e.pending {
        return Err(Refused::Conflict);
    }
    Ok(e)
}

/// Hold what a pending batch recorded, weighed against the conversations as
/// they are now: one held since, released by a person since its message
/// came, or no longer held by the rules stays out. Returns how many were
/// held.
pub fn confirm_batch<S: RejudgeStore>(
    store: &S,
    observation: &str,
    actor: &str,
) -> Result<u64, Refused> {
    let mut e = pending_entry(store, observation)?;
    let recorded: Vec<Candidate> = candidates(store, observation)
        .into_iter()
        .filter(|c| {
            store
                .ledger_verdict(&c.user, &c.mid)
                .ok()
                .flatten()
                .is_some_and(|v| v.holds)
        })
        .collect();
    let held = hold_all(store, observation, &recorded);
    e.pending = false;
    e.confirmed_at = now();
    if let Err(error) = put_entry(store, observation, &e) {
        tracing::error!(%error, %observation, "rejudge: the confirmed batch was not marked");
    }
    forget(store, observation);
    let detail = format!(
        "batch={observation} would_hold={} held={held} rules={}",
        e.would_hold,
        e.rules.join(",")
    );
    audit(store, actor, "rejudge.batch_confirm", observation, &detail);
    Ok(held)
}

/// Hold nothing of a pending batch. Returns how many it would have held.
pub fn discard_batch<S: RejudgeStore>(
    store: &S,
    observation: &str,
    actor: &str,
) -> Result<u64, Refused> {
    let mut e = pending_entry(store, observation)?;
    // their verdicts read "hold": the mark lets a later observation offer them
    for c in candidates(store, observation) {
        if let Err(error) = leave_unheld(store, &c.user, &c.mid, observation) {
            tracing::warn!(%error, user = %c.user, tid = %c.tid,
                "rejudge: a discarded conversation was not marked; no later observation offers it");
        }
    }
    e.pending = false;
    e.discarded_at = now();
    if let Err(error) = put_entry(store, observation, &e) {
        tracing::error!(%error, %observation, "rejudge: the discarded batch was not marked");
    }
    forget(store, observation);
    let detail = format!(
        "batch={observation} would_hold={} rules={}",
        e.would_hold,
        e.rules.join(",")
    );
    audit(store, actor, "rejudge.batch_discard", observation, &detail);
    Ok(e.would_hold)
}

/// Put every conversation of one batch back, each audited. A pending or a
/// discarded batch holds nothing and is refused, and so is one already put
/// back: its time and its audit line say when that was, once.
pub fn release_batch<S: RejudgeStore>(
    store: &S,
    observation: &str,
    actor: &str,
) -> Result<u64, Refused> {
    let mut e = entry(store, observation).ok_or(Refused::Missing)?;
    if e.pending || e.discarded_at != 0 || e.released_at != 0 {
        return Err(Refused::Conflict);
    }
    let members = store
        .state_smembers(&batch_key(observation))
        .unwrap_or_default();
    let mut released = 0;
    let conversations = members.len();
    // the rules the batch held for, for the line that says what was put back
    let mut batch_rules = std::collections::BTreeSet::new();
    let scope = format!("batch={observation}");
    let mut conn = store.net_conn();
    for m in members {
        let Some((user, tid)) = m.split_once('\t') else {
            continue;
        };
        // only what this batch still holds: a conversation released since,
        // or taken again by a later round, is not this batch's to move
        let still = store
            .state_hget(&recalled_key(user, tid), "observation")
            .ok()
            .flatten()
            .is_some_and(|o| o == observation.as_bytes());
        if !still {
            continue;
        }
        // read before `note_released` clears the recall mark
        let rules = rules_of(store.state_hget(&recalled_key(user, tid), "rules"));
        if store.release(user, tid).unwrap_or(false) {
            note_released(store, user, tid);
            if let Err(error) =
                facts::record_release(store, user, tid, facts::BATCH, actor, "", &scope)
            {
                tracing::error!(%error, %user, %tid, "rejudge: the batch release was not recorded");
            }
            released += 1;
            batch_rules.extend(rules.iter().cloned());
            if let Some(c) = conn.as_mut() {
                let detail = format!(
                    "user={user} observation={observation} rules={} reason=batch put back",
                    rules.join(",")
                );
                let _ = mailrs_core_sidestate::families::audit::append(
                    c,
                    actor,
                    "rejudge.release",
                    tid,
                    &detail,
                    now(),
                );
            }
        }
    }
    // the batch as a whole: whose decision, which batch, the rules it held
    // for, and how many it covered, so a put-back can be traced afterwards
    let detail = format!(
        "{scope} conversations={conversations} released={released} rules={}",
        batch_rules.into_iter().collect::<Vec<_>>().join(",")
    );
    audit(store, actor, "rejudge.batch_release", observation, &detail);
    e.released_at = now();
    if let Err(error) = put_entry(store, observation, &e) {
        tracing::error!(%error, %observation, "rejudge: the released batch was not marked");
    }
    Ok(released)
}
