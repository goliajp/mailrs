//! The batches re-judge rounds took back, for the administrator to list and
//! put back whole.

use super::*;

/// One batch, for the administrator.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub(crate) struct Batch {
    pub(crate) observation: String,
    pub(crate) at: i64,
    pub(crate) conversations: u64,
    pub(crate) released_at: i64,
}

/// Every batch, newest first.
pub(crate) fn batches(state: &FastcoreState) -> Vec<Batch> {
    let store = state.mailbox.store_ref();
    let mut out: Vec<Batch> = store
        .hgetall(BATCHES)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(obs, entry)| {
            let observation = String::from_utf8(obs).ok()?;
            let v: serde_json::Value = serde_json::from_slice(&entry).ok()?;
            let conversations = store
                .smembers(batch_key(&observation).as_bytes())
                .map(|m| m.len() as u64)
                .unwrap_or(0);
            Some(Batch {
                at: v["at"].as_i64().unwrap_or(0),
                released_at: v["released_at"].as_i64().unwrap_or(0),
                observation,
                conversations,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        b.at.cmp(&a.at)
            .then_with(|| a.observation.cmp(&b.observation))
    });
    out
}

/// Put every conversation of one batch back, each audited. `None` when
/// there is no such batch.
pub(crate) fn release_batch(state: &FastcoreState, observation: &str, actor: &str) -> Option<u64> {
    let store = state.mailbox.store_ref();
    let entry = store.hget(BATCHES, observation.as_bytes()).ok().flatten()?;
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
    if let Some(c) = conn.as_mut() {
        let detail = format!(
            "{scope} conversations={conversations} released={released} rules={}",
            batch_rules.into_iter().collect::<Vec<_>>().join(",")
        );
        if let Err(error) = mailrs_core_sidestate::families::audit::append(
            c,
            actor,
            "rejudge.batch_release",
            observation,
            &detail,
            now(),
        ) {
            tracing::warn!(%error, %observation, "rejudge: the batch audit line was not written");
        }
    }
    let mut v: serde_json::Value = serde_json::from_slice(&entry).unwrap_or_default();
    v["released_at"] = serde_json::json!(now());
    let _ = store.hset(
        BATCHES,
        &[(observation.as_bytes(), v.to_string().as_bytes())],
    );
    Some(released)
}
