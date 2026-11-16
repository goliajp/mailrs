//! Acting on a re-judged verdict: a conversation the rules now hold is
//! taken back into Review, whoever had read it, and its reader is told.
//!
//! Taking back only ever holds: nothing is deleted, and each observation's
//! round is one batch an administrator can put back whole. A conversation
//! a person put back is taken again only for mail that arrived after they
//! did, and then only for a rule it was not put back despite.
//!
//! A round gathers what it would hold and decides once all of the
//! observation's mail is judged (`settle`): up to `MAILRS_REJUDGE_HOLD_CAP`
//! conversations are held at once; more wait, held by nobody, as a
//! pending batch a super administrator confirms or discards.
//!
//! Off unless `MAILRS_REJUDGE_ACT=on`: until then a round records verdicts
//! and logs what it would do.
//!
//! A round that left mail the rules hold unheld — its batch discarded, or
//! judged while acting was off — marks each such message with its
//! observation. The verdict already reads "hold", so a later round would
//! find nothing changed; the mark lets a later, different observation
//! offer the message again, and keeps the marking round from doing so.

use super::facts;
use super::store::RejudgeStore;

mod batches;
mod settle;
mod tell;
mod unheld;
pub use batches::{Batch, Refused, batches, confirm_batch, discard_batch, release_batch};
pub use settle::{offer, settle};
pub use unheld::{leave_unheld, unheld_by_another};

/// The switch: `on` acts, anything else only logs.
pub const ACT_ENV: &str = "MAILRS_REJUDGE_ACT";

/// Every batch: observation id → JSON `{at, released_at, pending,
/// would_hold, rules, discarded_at, confirmed_at}`.
const BATCHES: &str = "rejudge:batches";

pub fn acting() -> bool {
    std::env::var(ACT_ENV).is_ok_and(|v| v.trim() == "on")
}

/// The conversations one observation's round took back, as `user\ttid`.
fn batch_key(observation: &str) -> String {
    format!("rejudge:batch:{observation}")
}

/// Which batch took a conversation back, and for which rules.
fn recalled_key(user: &str, tid: &str) -> String {
    format!("rejudge:recalled:{user}:{tid}")
}

/// The rules a person put a conversation back despite.
fn released_key(user: &str, tid: &str) -> String {
    format!("rejudge:released:{user}:{tid}")
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// One conversation the rules now hold, as a round keeps it until it
/// decides, and as a pending batch keeps it until someone does.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Candidate {
    pub user: String,
    pub tid: String,
    pub mid: String,
    pub from: String,
    pub subject: String,
    pub rules: Vec<String>,
    pub verdict: mailrs_inbound::FraudVerdict,
    /// When the message arrived, from our receiver's trace.
    pub arrived: Option<i64>,
}

/// Whether `c` may be taken now: not held already, not put back before
/// for the same rules, not released by a person after its message came.
fn may_take<S: RejudgeStore>(store: &S, c: &Candidate) -> Result<bool, String> {
    let released = rules_of(store.state_hget(&released_key(&c.user, &c.tid), "rules"));
    if !released.is_empty() && c.rules.iter().all(|rule| released.contains(rule)) {
        tracing::info!(user = %c.user, tid = %c.tid, rules = ?c.rules,
            "rejudge: put back before for these rules; left where it is");
        return Ok(false);
    }
    // a person released it after this message came: it stays released
    if !facts::may_hold_again(store, &c.user, &c.tid, c.arrived)? {
        tracing::info!(user = %c.user, tid = %c.tid,
            "rejudge: a person released it after this message came; left where it is");
        return Ok(false);
    }
    Ok(store
        .conversation(&c.user, &c.tid)
        .map_err(|e| e.to_string())?
        .is_some_and(|row| !row.held))
}

/// Take `c` back into Review, in `observation`'s batch. Returns whether it
/// was taken; the caller tells the reader, once for the round.
fn recall<S: RejudgeStore>(store: &S, c: &Candidate, observation: &str) -> Result<bool, String> {
    if !may_take(store, c)? {
        return Ok(false);
    }
    let held_now = super::holding::apply(store, &c.user, &c.tid, &c.mid, &c.verdict, "recall")?;
    if !held_now {
        return Ok(false);
    }
    remember(store, &c.user, &c.tid, observation, &c.rules)?;
    unheld::taken(store, &c.user, &c.mid)?;
    Ok(true)
}

/// Take a conversation back into Review because a report found it to be a
/// copy of a reported scam, in the batch an administrator puts back whole.
/// The reader is told by the report (`crate::scam_report`). False when it
/// was already held, or when a person released the conversation after
/// `arrived` (when the reported copy came): as for a re-judge, only mail
/// that came after a release may take it back.
pub fn hold_reported<S: RejudgeStore>(
    store: &S,
    user: &str,
    tid: &str,
    arrived: Option<i64>,
    batch: &str,
) -> Result<bool, String> {
    let row = store
        .conversation(user, tid)
        .map_err(|e| e.to_string())?
        .ok_or("thread membership disappeared")?;
    if row.held {
        return Ok(false);
    }
    if !facts::may_hold_again(store, user, tid, arrived)? {
        tracing::info!(%user, %tid,
            "report: a person released it after this copy came; left where it is");
        return Ok(false);
    }
    if !store
        .hold(user, tid, facts::REPORT, "")
        .map_err(|e| e.to_string())?
    {
        return Err("thread membership disappeared".into());
    }
    remember(store, user, tid, batch, &["reported".to_string()])?;
    Ok(true)
}

/// Count `tid` into `observation`'s batch and mark which batch holds it.
fn remember<S: RejudgeStore>(
    store: &S,
    user: &str,
    tid: &str,
    observation: &str,
    rules: &[String],
) -> Result<(), String> {
    let at = now();
    let rules = serde_json::to_string(rules).map_err(|e| e.to_string())?;
    store
        .state_sadd(&batch_key(observation), &format!("{user}\t{tid}"))
        .map_err(|e| e.to_string())?;
    if batches::entry(store, observation).is_none() {
        batches::put_entry(store, observation, &batches::Entry::new(at, false, 0, &[]))?;
    }
    let key = recalled_key(user, tid);
    for (field, value) in [
        ("observation", observation.to_string()),
        ("rules", rules),
        ("at", at.to_string()),
    ] {
        store
            .state_hset(&key, field, value.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// A rejudge-held conversation the rules no longer hold goes back where
/// it was. A conversation held on arrival is not this module's to release,
/// and neither is one a person's report holds.
pub fn unrecall<S: RejudgeStore>(
    store: &S,
    user: &str,
    tid: &str,
    rules_version: &str,
) -> Result<bool, String> {
    let key = recalled_key(user, tid);
    let Some(observation) = store.state_hget(&key, "observation").ok().flatten() else {
        return Ok(false);
    };
    if facts::held_by_report(store, user, tid)? {
        return Ok(false);
    }
    let ok = store.release(user, tid).map_err(|e| e.to_string())?;
    store.state_del(&key).map_err(|e| e.to_string())?;
    if ok {
        facts::record_release(
            store,
            user,
            tid,
            facts::REJUDGE,
            facts::SYSTEM,
            rules_version,
            "",
        )?;
    }
    if let Some(mut conn) = store.net_conn() {
        let detail = format!(
            "user={user} observation={} reason=the rules no longer hold it",
            String::from_utf8_lossy(&observation)
        );
        let _ = mailrs_core_sidestate::families::audit::append(
            &mut conn,
            "rejudge",
            "rejudge.release",
            tid,
            &detail,
            now(),
        );
    }
    Ok(ok)
}

/// A person put `tid` back: remember the rules it was held for, so the
/// same rules do not take it again. The rules are every rule the ledger
/// holds against the conversation's messages for this reader.
pub fn note_released<S: RejudgeStore>(store: &S, user: &str, tid: &str) {
    let mut rules: Vec<String> = store
        .thread_message_ids(user, tid)
        .unwrap_or_default()
        .iter()
        .filter_map(|mid| store.ledger_verdict(user, mid).ok().flatten())
        .flat_map(|v| v.rules)
        .collect();
    let recalled = recalled_key(user, tid);
    rules.extend(rules_of(store.state_hget(&recalled, "rules")));
    rules.sort();
    rules.dedup();
    if let Err(e) = store.state_del(&recalled) {
        tracing::warn!(error = %e, %user, %tid, "rejudge: the recall mark was not cleared");
    }
    if rules.is_empty() {
        return;
    }
    let json = serde_json::to_string(&rules).unwrap_or_default();
    if let Err(e) = store.state_hset(&released_key(user, tid), "rules", json.as_bytes()) {
        tracing::warn!(error = %e, %user, %tid, "rejudge: the release was not remembered");
    }
}

fn rules_of<E>(stored: Result<Option<Vec<u8>>, E>) -> Vec<String> {
    stored
        .ok()
        .flatten()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
