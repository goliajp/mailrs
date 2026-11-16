//! Holding a conversation on a verdict reached after it was filed: the
//! verdict stored beside the message, the hold and its fact, the audit
//! line. What the reader has read is left as it is: held mail is out of
//! the badge, and a release returns the conversation as they left it.

use mailrs_inbound::FraudVerdict;

use super::facts;
use super::store::RejudgeStore;

/// Hold `tid` for `user` on `verdict` for message `mid`. Returns whether it
/// was not held before. Where the message was filed on arrival, and why,
/// still stands: the verdict judges the content again, not the filing.
pub fn apply<S: RejudgeStore>(
    store: &S,
    user: &str,
    tid: &str,
    mid: &str,
    verdict: &FraudVerdict,
    source: &str,
) -> Result<bool, String> {
    let row = store
        .conversation(user, tid)
        .map_err(|e| e.to_string())?
        .ok_or("thread membership disappeared")?;
    let stored = store.fraud_verdict(mid).map_err(|e| e.to_string())?;
    let mut verdict = verdict.clone();
    if let Some(old) = stored
        .as_deref()
        .and_then(|j| serde_json::from_str::<FraudVerdict>(j).ok())
        && verdict.placement.is_empty()
    {
        verdict.placement = old.placement;
    }
    let json = serde_json::to_string(&verdict).map_err(|e| e.to_string())?;
    if stored.as_deref() != Some(&json) {
        store
            .set_fraud_verdict(mid, &json)
            .map_err(|e| e.to_string())?;
    }
    if row.held {
        return Ok(false);
    }
    if !store
        .hold(user, tid, source, &verdict.rules_version)
        .map_err(|e| e.to_string())?
    {
        return Err("thread membership disappeared".into());
    }
    facts::audit_hold(
        store,
        user,
        tid,
        source,
        &verdict.rules_version,
        &facts::verdict_rules(&verdict),
    );
    Ok(true)
}
