//! How much memory the fact ledger would take, measured on real mail.
//!
//! Read-only on the store: the newest message of up to `limit`
//! conversations is read, and the entries the ledger design gives one
//! message are written into a fresh in-memory store that is dropped
//! afterwards. The answer is the growth of this process's resident memory
//! across the writes, and the raw bytes written, for comparison.
//!
//! What it writes is what delivery will write: the ledger tables as
//! `mailrs_mailbox_kevy::ledger` declares them (the message, one row per
//! link host and per attachment, each indexed), the verdict beside them,
//! and the running relationship with the sender.

use super::fraud_rescan::reading::newest_raw;
use super::prelude::*;

#[derive(Debug, serde::Deserialize)]
pub(crate) struct EstimateQuery {
    #[serde(default = "default_limit")]
    limit: u64,
    /// Stops after this many seconds and reports what it measured.
    #[serde(default = "default_max_secs")]
    max_secs: u64,
}

fn default_limit() -> u64 {
    10_000
}

fn default_max_secs() -> u64 {
    300
}

/// This process's resident memory, in bytes, from `/proc/self/status`.
fn rss() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

/// `POST /v1/admin/maintenance:ledger-estimate?limit=10000`
pub(crate) async fn ledger_estimate_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<EstimateQuery>,
) -> axum::response::Response {
    // the fraud rules' VM stays on the thread it was made on
    let budget = std::time::Duration::from_secs(q.max_secs);
    match super::hangup::run_blocking(move |h| estimate(&state, q.limit, budget, h)).await {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Default)]
struct Tally {
    messages: u64,
    raw_bytes: u64,
    keys: u64,
}

fn estimate(
    state: &Arc<FastcoreState>,
    limit: u64,
    budget: std::time::Duration,
    hangup: &super::hangup::Hangup,
) -> Result<serde_json::Value, String> {
    let started = std::time::Instant::now();
    let (mut walked, mut stopped_early) = (0u64, false);
    let targets = super::fraud_rescan::snapshot(state).map_err(|e| e.to_string())?;
    let mut fraud = super::fraud_rescan::FraudJudge::new(state);
    let before = rss();
    let ledger = mailrs_mailbox_kevy::KevyMailboxStore::new(std::sync::Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).map_err(|e| e.to_string())?,
    ));
    ledger.ensure_ledger_tables().map_err(|e| e.to_string())?;
    let mut t = Tally::default();
    let mut users = std::collections::HashSet::new();
    for (user, tid) in &targets {
        if t.messages >= limit {
            break;
        }
        if hangup.hung_up() || started.elapsed() >= budget {
            stopped_early = true;
            break;
        }
        walked += 1;
        let Some((mid, raw)) = newest_raw(state, user, tid) else {
            continue;
        };
        t.messages += 1;
        users.insert(user.clone());
        let facts = crate::ledger::facts_of(user, &mid, &raw);
        let verdict = match fraud.judge(state, &mid, &raw) {
            Ok(j) => mailrs_mailbox_kevy::ledger::LedgerVerdict {
                rules: j.findings.rules().iter().map(|r| r.to_string()).collect(),
                score: j.findings.score(),
                rules_version: j.version.clone(),
                judged_at: facts.arrived,
            },
            Err(_) => mailrs_mailbox_kevy::ledger::LedgerVerdict::default(),
        };
        t.raw_bytes += (facts.message_id.len()
            + facts.user.len()
            + facts.from_addr.len()
            + facts.from_domain.len()
            + facts.reply_to.len()
            + facts.auth.len()
            + facts
                .link_hosts
                .iter()
                .map(|h| h.len() + mid.len())
                .sum::<usize>()
            + facts
                .attachment_sha256
                .iter()
                .map(|h| h.len() + mid.len())
                .sum::<usize>()
            + verdict.rules.iter().map(|r| r.len()).sum::<usize>()
            + verdict.rules_version.len()) as u64;
        t.keys += 2 + facts.link_hosts.len() as u64 + facts.attachment_sha256.len() as u64;
        ledger.record_ledger(&facts).map_err(|e| e.to_string())?;
        ledger
            .record_verdict(user, &mid, &verdict)
            .map_err(|e| e.to_string())?;
        let name = mailrs_inbound::from_header(&raw);
        ledger
            .record_inbound_relationship(user, &facts.from_addr, &name, facts.arrived)
            .map_err(|e| e.to_string())?;
        t.keys += 1;
    }
    let after = rss();
    drop(ledger);
    let dropped = rss();
    let delta = match (before, after) {
        (Some(b), Some(a)) => Some(a.saturating_sub(b)),
        _ => None,
    };
    let per_10k = delta
        .filter(|_| t.messages > 0)
        .map(|d| d * 10_000 / t.messages);
    Ok(serde_json::json!({
        "walked": walked,
        "stopped_early": stopped_early,
        "messages": t.messages,
        "users": users.len(),
        "keys": t.keys,
        "raw_bytes": t.raw_bytes,
        "raw_bytes_per_message": t.raw_bytes.checked_div(t.messages),
        "rss_before": before,
        "rss_after": after,
        "rss_after_drop": dropped,
        "rss_growth": delta,
        "rss_growth_per_10k_messages": per_10k,
    }))
}

#[cfg(test)]
mod tests {
    use super::super::hangup::{Hangup, seed};
    use super::*;

    #[test]
    fn the_estimate_stops_soon_after_the_caller_hangs_up() {
        let state = crate::tests::fresh_state();
        seed(&state, "u@x", 5);
        let budget = std::time::Duration::from_secs(60);
        let all = estimate(&state, 100, budget, &Hangup::never()).unwrap();
        assert_eq!(all["walked"], 5, "{all}");
        assert_eq!(all["stopped_early"], false, "{all}");
        let cut = estimate(&state, 100, budget, &Hangup::after(2)).unwrap();
        assert_eq!(cut["walked"], 2, "{cut}");
        assert_eq!(cut["stopped_early"], true, "{cut}");
    }

    #[test]
    fn the_estimate_stops_at_its_time_budget() {
        let state = crate::tests::fresh_state();
        seed(&state, "u@x", 5);
        let out = estimate(&state, 100, std::time::Duration::ZERO, &Hangup::never()).unwrap();
        assert_eq!(out["walked"], 0, "{out}");
        assert_eq!(out["stopped_early"], true, "{out}");
    }
}
