//! Acting on a re-judged verdict: a conversation the rules now hold is
//! taken back into Review, whoever had read it, and its reader is told.
//!
//! Taking back only ever holds: nothing is deleted, and each observation's
//! round is one batch an administrator can put back whole. A conversation
//! a person put back is taken again only for mail that arrived after they
//! did, and then only for a rule it was not put back despite.
//!
//! Off unless `MAILRS_REJUDGE_ACT=on`: until then a round records verdicts
//! and logs what it would do.

use std::sync::Arc;

use mailrs_core_sidestate::families::recall_notices::{self, Recalled};

use crate::FastcoreState;

mod batches;
pub(crate) use batches::{batches, release_batch};

/// The switch: `on` acts, anything else only logs.
pub(crate) const ACT_ENV: &str = "MAILRS_REJUDGE_ACT";

/// Every batch: observation id → JSON `{at, released_at}`.
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

/// One conversation the rules now hold.
pub(crate) struct Recall<'a> {
    pub(crate) user: &'a str,
    pub(crate) tid: &'a str,
    pub(crate) mid: &'a str,
    pub(crate) from: &'a str,
    pub(crate) subject: &'a str,
    pub(crate) rules: Vec<String>,
    pub(crate) observation: &'a str,
    pub(crate) verdict: &'a mailrs_inbound::FraudVerdict,
    /// When the message arrived, from our receiver's trace.
    pub(crate) arrived: Option<i64>,
}

/// Take `r` back into Review. Returns whether it was taken (false when it
/// was already held, or put back before for the same rules).
pub(crate) fn recall(state: &Arc<FastcoreState>, r: &Recall<'_>) -> Result<bool, String> {
    let store = state.mailbox.store_ref();
    let released = rules_of(store.hget(released_key(r.user, r.tid).as_bytes(), b"rules"));
    if !released.is_empty() && r.rules.iter().all(|rule| released.contains(rule)) {
        tracing::info!(user = %r.user, tid = %r.tid, rules = ?r.rules,
            "rejudge: put back before for these rules; left where it is");
        return Ok(false);
    }
    // a person released it after this message came: it stays released
    if !crate::hold_facts::may_hold_again(state, r.user, r.tid, r.arrived)? {
        tracing::info!(user = %r.user, tid = %r.tid,
            "rejudge: a person released it after this message came; left where it is");
        return Ok(false);
    }
    let held_now = crate::maintenance::fraud_rescan::holding::apply(
        state, r.user, r.tid, r.mid, r.verdict, "recall",
    )?;
    if !held_now {
        return Ok(false);
    }
    let at = now();
    let rules = serde_json::to_string(&r.rules).map_err(|e| e.to_string())?;
    let member = format!("{}\t{}", r.user, r.tid);
    store
        .sadd(batch_key(r.observation).as_bytes(), &[member.as_bytes()])
        .map_err(|e| e.to_string())?;
    if store
        .hget(BATCHES, r.observation.as_bytes())
        .ok()
        .flatten()
        .is_none()
    {
        let entry = serde_json::json!({ "at": at, "released_at": 0 }).to_string();
        store
            .hset(BATCHES, &[(r.observation.as_bytes(), entry.as_bytes())])
            .map_err(|e| e.to_string())?;
    }
    let at_s = at.to_string();
    store
        .hset(
            recalled_key(r.user, r.tid).as_bytes(),
            &[
                (b"observation".as_slice(), r.observation.as_bytes()),
                (b"rules", rules.as_bytes()),
                (b"at", at_s.as_bytes()),
            ],
        )
        .map_err(|e| e.to_string())?;
    tell(state, r, at);
    Ok(true)
}

/// The audit line, the reader's notice, the webhook and the push, each
/// logged on failure: the conversation is held whether or not they land.
fn tell(state: &Arc<FastcoreState>, r: &Recall<'_>, at: i64) {
    let Some(mut conn) = state.net_conn() else {
        tracing::warn!(user = %r.user, tid = %r.tid, "rejudge: no shared store; the reader was not told");
        return;
    };
    let detail = format!(
        "user={} observation={} rules={}",
        r.user,
        r.observation,
        r.rules.join(",")
    );
    if let Err(e) = mailrs_core_sidestate::families::audit::append(
        &mut conn,
        "rejudge",
        "rejudge.recall",
        r.tid,
        &detail,
        at,
    ) {
        tracing::warn!(error = %e, tid = %r.tid, "rejudge: the recall was not audited");
    }
    let notice = Recalled {
        thread_id: r.tid.to_string(),
        subject: r.subject.to_string(),
        rules: r.rules.clone(),
        observation: r.observation.to_string(),
        at,
    };
    if let Err(e) = recall_notices::push(&mut conn, r.user, &notice) {
        tracing::warn!(error = %e, tid = %r.tid, "rejudge: the reader's notice was not written");
    }
    let event = mailrs_core_sidestate::families::webhook_events::Event {
        kind: mailrs_core_sidestate::families::webhook_events::kinds::MAIL_HELD,
        user: r.user,
        sender: Some(r.from),
        thread_id: Some(r.tid),
        data: serde_json::json!({
            "message_id": r.mid,
            "subject": r.subject,
            "snippet": "",
            "reason": "rejudged",
            "rules": r.rules,
            "observation": r.observation,
        }),
    };
    mailrs_core_sidestate::families::webhook_events::enqueue_logged(&mut conn, &event, at);
    mailrs_intake::push::notify_recalled(Some(&mut conn), r.user, r.tid, r.from, r.subject);
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
