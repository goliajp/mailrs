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

use std::sync::Arc;

use crate::FastcoreState;

mod batches;
mod settle;
mod tell;
pub(crate) use batches::{Refused, batches, confirm_batch, discard_batch, release_batch};
pub(crate) use settle::{offer, settle};

/// The switch: `on` acts, anything else only logs.
pub(crate) const ACT_ENV: &str = "MAILRS_REJUDGE_ACT";

/// Every batch: observation id → JSON `{at, released_at, pending,
/// would_hold, rules, discarded_at}`.
const BATCHES: &[u8] = b"rejudge:batches";

pub(crate) fn acting() -> bool {
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
pub(crate) struct Candidate {
    pub(crate) user: String,
    pub(crate) tid: String,
    pub(crate) mid: String,
    pub(crate) from: String,
    pub(crate) subject: String,
    pub(crate) rules: Vec<String>,
    pub(crate) verdict: mailrs_inbound::FraudVerdict,
    /// When the message arrived, from our receiver's trace.
    pub(crate) arrived: Option<i64>,
}

/// Whether `c` may be taken now: not held already, not put back before
/// for the same rules, not released by a person after its message came.
fn may_take(state: &FastcoreState, c: &Candidate) -> Result<bool, String> {
    let store = state.mailbox.store_ref();
    let released = rules_of(store.hget(released_key(&c.user, &c.tid).as_bytes(), b"rules"));
    if !released.is_empty() && c.rules.iter().all(|rule| released.contains(rule)) {
        tracing::info!(user = %c.user, tid = %c.tid, rules = ?c.rules,
            "rejudge: put back before for these rules; left where it is");
        return Ok(false);
    }
    // a person released it after this message came: it stays released
    if !crate::hold_facts::may_hold_again(state, &c.user, &c.tid, c.arrived)? {
        tracing::info!(user = %c.user, tid = %c.tid,
            "rejudge: a person released it after this message came; left where it is");
        return Ok(false);
    }
    Ok(state
        .mailbox
        .get_thread_for_user(&c.user, &c.tid)
        .map_err(|e| e.to_string())?
        .is_some_and(|row| !row.quarantined))
}

/// Take `c` back into Review, in `observation`'s batch. Returns whether it
/// was taken; the caller tells the reader, once for the round.
fn recall(state: &Arc<FastcoreState>, c: &Candidate, observation: &str) -> Result<bool, String> {
    if !may_take(state, c)? {
        return Ok(false);
    }
    let held_now = crate::maintenance::fraud_rescan::holding::apply(
        state, &c.user, &c.tid, &c.mid, &c.verdict, "recall",
    )?;
    if !held_now {
        return Ok(false);
    }
    remember(state, &c.user, &c.tid, observation, &c.rules)?;
    Ok(true)
}

/// Take a conversation back into Review because a report found it to be a
/// copy of a reported scam, in the batch an administrator puts back whole.
/// The reader is told by the report (`mailrs_intake::scam_report`). False
/// when it was already held, or when a person released the conversation
/// after `arrived` (when the reported copy came): as for a re-judge, only
/// mail that came after a release may take it back.
pub(crate) fn hold_reported(
    state: &Arc<FastcoreState>,
    user: &str,
    tid: &str,
    arrived: Option<i64>,
    batch: &str,
) -> Result<bool, String> {
    let row = state
        .mailbox
        .get_thread_for_user(user, tid)
        .map_err(|e| e.to_string())?
        .ok_or("thread membership disappeared")?;
    if row.quarantined {
        return Ok(false);
    }
    if !crate::hold_facts::may_hold_again(state, user, tid, arrived)? {
        tracing::info!(%user, %tid,
            "report: a person released it after this copy came; left where it is");
        return Ok(false);
    }
    if !state
        .mailbox
        .set_quarantined(user, tid, true)
        .map_err(|e| e.to_string())?
    {
        return Err("thread membership disappeared".into());
    }
    crate::hold_facts::record_hold_fact(state, user, tid, crate::hold_facts::REPORT, "")?;
    remember(state, user, tid, batch, &["reported".to_string()])?;
    Ok(true)
}

/// Count `tid` into `observation`'s batch and mark which batch holds it.
fn remember(
    state: &Arc<FastcoreState>,
    user: &str,
    tid: &str,
    observation: &str,
    rules: &[String],
) -> Result<(), String> {
    let store = state.mailbox.store_ref();
    let at = now();
    let rules = serde_json::to_string(rules).map_err(|e| e.to_string())?;
    let member = format!("{user}\t{tid}");
    store
        .sadd(batch_key(observation).as_bytes(), &[member.as_bytes()])
        .map_err(|e| e.to_string())?;
    if batches::entry(state, observation).is_none() {
        batches::put_entry(state, observation, &batches::Entry::new(at, false, 0, &[]))?;
    }
    let at_s = at.to_string();
    store
        .hset(
            recalled_key(user, tid).as_bytes(),
            &[
                (b"observation".as_slice(), observation.as_bytes()),
                (b"rules", rules.as_bytes()),
                (b"at", at_s.as_bytes()),
            ],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// A rejudge-held conversation the rules no longer hold goes back where
/// it was. A conversation held on arrival is not this module's to release.
pub(crate) fn unrecall(
    state: &Arc<FastcoreState>,
    user: &str,
    tid: &str,
    rules_version: &str,
) -> Result<bool, String> {
    let store = state.mailbox.store_ref();
    let key = recalled_key(user, tid);
    let Some(observation) = store.hget(key.as_bytes(), b"observation").ok().flatten() else {
        return Ok(false);
    };
    let ok = state
        .mailbox
        .set_quarantined(user, tid, false)
        .map_err(|e| e.to_string())?;
    store.del(&[key.as_bytes()]).map_err(|e| e.to_string())?;
    if ok {
        crate::hold_facts::record_release(
            state,
            user,
            tid,
            crate::hold_facts::REJUDGE,
            crate::hold_facts::SYSTEM,
            rules_version,
            "",
        )?;
    }
    if let Some(mut conn) = state.net_conn() {
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
pub(crate) fn note_released(state: &FastcoreState, user: &str, tid: &str) {
    let mut rules: Vec<String> = state
        .mailbox
        .user_thread_message_ids(user, tid)
        .unwrap_or_default()
        .iter()
        .filter_map(|mid| state.mailbox.ledger_verdict(user, mid).ok().flatten())
        .flat_map(|v| v.rules)
        .collect();
    let store = state.mailbox.store_ref();
    let recalled = recalled_key(user, tid);
    rules.extend(rules_of(store.hget(recalled.as_bytes(), b"rules")));
    rules.sort();
    rules.dedup();
    if let Err(e) = store.del(&[recalled.as_bytes()]) {
        tracing::warn!(error = %e, %user, %tid, "rejudge: the recall mark was not cleared");
    }
    if rules.is_empty() {
        return;
    }
    let json = serde_json::to_string(&rules).unwrap_or_default();
    if let Err(e) = store.hset(
        released_key(user, tid).as_bytes(),
        &[(b"rules".as_slice(), json.as_bytes())],
    ) {
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
