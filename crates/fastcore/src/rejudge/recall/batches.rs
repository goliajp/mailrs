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
        }
    }
}

pub(super) fn entry(state: &FastcoreState, observation: &str) -> Option<Entry> {
    let raw = state
        .mailbox
        .store_ref()
        .hget(BATCHES, observation.as_bytes())
        .ok()
        .flatten()?;
    serde_json::from_slice(&raw).ok()
}

pub(super) fn put_entry(state: &FastcoreState, observation: &str, e: &Entry) -> Result<(), String> {
    let json = serde_json::to_vec(e).map_err(|e| e.to_string())?;
    state
        .mailbox
        .store_ref()
        .hset(BATCHES, &[(observation.as_bytes(), json.as_slice())])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// One batch, for the administrator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Batch {
    pub(crate) observation: String,
    pub(crate) at: i64,
    pub(crate) conversations: u64,
    pub(crate) released_at: i64,
    pub(crate) pending: bool,
    pub(crate) would_hold: u64,
    pub(crate) rules: Vec<String>,
    pub(crate) discarded_at: i64,
}

/// Why a decision on a batch was refused.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Refused {
    /// No such batch.
    Missing,
    /// The batch is not in the state the decision is for: a pending one
    /// cannot be put back, a decided one cannot be decided again.
    Conflict,
}

/// Every batch, newest first.
pub(crate) fn batches(state: &FastcoreState) -> Vec<Batch> {
    let store = state.mailbox.store_ref();
    let mut out: Vec<Batch> = store
        .hgetall(BATCHES)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(obs, raw)| {
            let observation = String::from_utf8(obs).ok()?;
            let e: Entry = serde_json::from_slice(&raw).ok()?;
            let conversations = store
                .smembers(batch_key(&observation).as_bytes())
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
            })
        })
        .collect();
    out.sort_by(|a, b| {
        b.at.cmp(&a.at)
            .then_with(|| a.observation.cmp(&b.observation))
    });
    out
}

fn audit(state: &FastcoreState, actor: &str, action: &str, observation: &str, detail: &str) {
    let Some(mut c) = state.net_conn() else {
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

fn pending_entry(state: &FastcoreState, observation: &str) -> Result<Entry, Refused> {
    let e = entry(state, observation).ok_or(Refused::Missing)?;
    if !e.pending {
        return Err(Refused::Conflict);
    }
    Ok(e)
}

/// Hold what a pending batch recorded, weighed against the conversations as
/// they are now: one held since, released by a person since its message
/// came, or no longer held by the rules stays out. Returns how many were
/// held.
pub(crate) fn confirm_batch(
    state: &Arc<FastcoreState>,
    observation: &str,
    actor: &str,
) -> Result<u64, Refused> {
    let mut e = pending_entry(state, observation)?;
    let recorded: Vec<Candidate> = candidates(state, observation)
        .into_iter()
        .filter(|c| {
            state
                .mailbox
                .ledger_verdict(&c.user, &c.mid)
                .ok()
                .flatten()
                .is_some_and(|v| v.holds)
        })
        .collect();
    let held = hold_all(state, observation, &recorded);
    e.pending = false;
    if let Err(error) = put_entry(state, observation, &e) {
        tracing::error!(%error, %observation, "rejudge: the confirmed batch was not marked");
    }
    forget(state, observation);
    let detail = format!(
        "batch={observation} would_hold={} held={held} rules={}",
        e.would_hold,
        e.rules.join(",")
    );
    audit(state, actor, "rejudge.batch_confirm", observation, &detail);
    Ok(held)
}

/// Hold nothing of a pending batch. Returns how many it would have held.
pub(crate) fn discard_batch(
    state: &FastcoreState,
    observation: &str,
    actor: &str,
) -> Result<u64, Refused> {
    let mut e = pending_entry(state, observation)?;
    e.pending = false;
    e.discarded_at = now();
    if let Err(error) = put_entry(state, observation, &e) {
        tracing::error!(%error, %observation, "rejudge: the discarded batch was not marked");
    }
    forget(state, observation);
    let detail = format!(
        "batch={observation} would_hold={} rules={}",
        e.would_hold,
        e.rules.join(",")
    );
    audit(state, actor, "rejudge.batch_discard", observation, &detail);
    Ok(e.would_hold)
}

/// Put every conversation of one batch back, each audited.
pub(crate) fn release_batch(
    state: &FastcoreState,
    observation: &str,
    actor: &str,
) -> Result<u64, Refused> {
    let mut e = entry(state, observation).ok_or(Refused::Missing)?;
    if e.pending {
        return Err(Refused::Conflict);
    }
    let store = state.mailbox.store_ref();
    let members = store
        .smembers(batch_key(observation).as_bytes())
        .unwrap_or_default();
    let mut released = 0;
    let conversations = members.len();
    // the rules the batch held for, for the line that says what was put back
    let mut batch_rules = std::collections::BTreeSet::new();
    let scope = format!("batch={observation}");
    let mut conn = state.net_conn();
    for m in members {
        let m = String::from_utf8_lossy(&m).to_string();
        let Some((user, tid)) = m.split_once('\t') else {
            continue;
        };
        // only what this batch still holds: a conversation released since,
        // or taken again by a later round, is not this batch's to move
        let still = store
            .hget(recalled_key(user, tid).as_bytes(), b"observation")
            .ok()
            .flatten()
            .is_some_and(|o| o == observation.as_bytes());
        if !still {
            continue;
        }
        // read before `note_released` clears the recall mark
        let rules = rules_of(store.hget(recalled_key(user, tid).as_bytes(), b"rules"));
        if state
            .mailbox
            .set_quarantined(user, tid, false)
            .unwrap_or(false)
        {
            note_released(state, user, tid);
            if let Err(error) = crate::hold_facts::record_release(
                state,
                user,
                tid,
                crate::hold_facts::BATCH,
                actor,
                "",
                &scope,
            ) {
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
    audit(state, actor, "rejudge.batch_release", observation, &detail);
    e.released_at = now();
    if let Err(error) = put_entry(state, observation, &e) {
        tracing::error!(%error, %observation, "rejudge: the released batch was not marked");
    }
    Ok(released)
}
