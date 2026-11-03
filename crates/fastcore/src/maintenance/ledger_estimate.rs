//! How much memory the fact ledger would take, measured on real mail.
//!
//! Read-only on the store: the newest message of up to `limit`
//! conversations is read, and the entries the ledger design gives one
//! message are written into a fresh in-memory store that is dropped
//! afterwards. The answer is the store's own byte count per family (the
//! families written one after another, the count read between them), and
//! the process's resident growth across the writes as a second reading.
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

/// One message's entries, read and judged before anything is measured.
struct Entry {
    facts: mailrs_mailbox_kevy::ledger::LedgerFacts,
    verdict: mailrs_mailbox_kevy::ledger::LedgerVerdict,
    name: String,
}

/// A fresh store holding nothing but what is written into it here.
fn fresh() -> Result<
    (
        Arc<kevy_embedded::Store>,
        mailrs_mailbox_kevy::KevyMailboxStore,
    ),
    String,
> {
    let store = Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).map_err(|e| e.to_string())?,
    );
    let ledger = mailrs_mailbox_kevy::KevyMailboxStore::new(store.clone());
    ledger.ensure_ledger_tables().map_err(|e| e.to_string())?;
    Ok((store, ledger))
}

/// Read and judge first, so the measured part is the writes alone: the
/// files, the parse and the rules' VM grow the heap too, and measured
/// together they hid the ledger's own size.
fn read_and_judge(
    state: &Arc<FastcoreState>,
    limit: u64,
    budget: std::time::Duration,
    hangup: &super::hangup::Hangup,
) -> Result<(Vec<Entry>, u64, bool, usize), String> {
    let started = std::time::Instant::now();
    let (mut walked, mut stopped_early) = (0u64, false);
    let targets = super::fraud_rescan::snapshot(state).map_err(|e| e.to_string())?;
    let mut fraud = super::fraud_rescan::FraudJudge::new(state);
    let mut users = std::collections::HashSet::new();
    let mut entries = Vec::new();
    for (user, tid) in &targets {
        if entries.len() as u64 >= limit {
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
        users.insert(user.clone());
        let facts = crate::ledger::facts_of(user, &mid, &raw);
        let verdict = match fraud.judge(state, user, &mid, &raw) {
            Ok(j) => crate::ledger::judged_verdict(&j.findings, &j.version, Vec::new()),
            Err(_) => mailrs_mailbox_kevy::ledger::LedgerVerdict::default(),
        };
        let name = mailrs_inbound::from_header(&raw);
        entries.push(Entry {
            facts,
            verdict,
            name,
        });
    }
    Ok((entries, walked, stopped_early, users.len()))
}

/// Bytes per 10,000 messages, from a total over `n`.
fn per_10k(bytes: u64, n: u64) -> Option<u64> {
    (n > 0).then(|| bytes * 10_000 / n)
}

fn estimate(
    state: &Arc<FastcoreState>,
    limit: u64,
    budget: std::time::Duration,
    hangup: &super::hangup::Hangup,
) -> Result<serde_json::Value, String> {
    let (entries, walked, stopped_early, users) = read_and_judge(state, limit, budget, hangup)?;
    let mut out = measure(&entries)?;
    out["walked"] = walked.into();
    out["stopped_early"] = stopped_early.into();
    out["users"] = users.into();
    Ok(out)
}

/// The writes alone, into fresh stores, measured.
fn measure(entries: &[Entry]) -> Result<serde_json::Value, String> {
    let n = entries.len() as u64;
    let links: u64 = entries
        .iter()
        .map(|e| e.facts.link_hosts.len() as u64)
        .sum();
    let files: u64 = entries
        .iter()
        .map(|e| e.facts.attachment_sha256.len() as u64)
        .sum();

    // Store one: the families one after another, the store's own count
    // read between them.
    let rss_before = rss();
    let (store, ledger) = fresh()?;
    let used = || store.info().used_memory;
    let empty = used();
    for e in entries {
        let bare = mailrs_mailbox_kevy::ledger::LedgerFacts {
            link_hosts: Vec::new(),
            attachment_sha256: Vec::new(),
            ..e.facts.clone()
        };
        ledger.record_ledger(&bare).map_err(|e| e.to_string())?;
    }
    let after_msg = used();
    for e in entries {
        ledger
            .record_verdict(&e.facts.user, &e.facts.message_id, &e.verdict)
            .map_err(|e| e.to_string())?;
    }
    let after_verdict = used();
    for e in entries {
        ledger
            .record_inbound_relationship(
                &e.facts.user,
                &e.facts.from_addr,
                &e.name,
                e.facts.arrived,
            )
            .map_err(|e| e.to_string())?;
    }
    let after_rel = used();
    let keys_one = store.info().keys as u64;
    let rss_after = rss();
    drop(ledger);
    drop(store);

    // Store two: whole facts, so links and files are the difference.
    let (store, ledger) = fresh()?;
    let empty_two = store.info().used_memory;
    let mut link_rows = 0u64;
    for e in entries {
        // the rows delivery would write: the hosts it indexes, and none for
        // mail past the retention
        let hosts = crate::ledger::indexed_link_hosts(&e.facts);
        let until = crate::ledger::links_until(&e.facts);
        if until > chrono::Utc::now().timestamp() {
            link_rows += hosts.len() as u64;
        }
        ledger
            .record_ledger_indexed(&e.facts, &hosts, Some(until))
            .map_err(|e| e.to_string())?;
    }
    let whole = store.info().used_memory - empty_two;
    drop(ledger);
    drop(store);

    let msg = after_msg - empty;
    let verdict = after_verdict - after_msg;
    let relationship = after_rel - after_verdict;
    let links_files = whole.saturating_sub(msg);
    let total = msg + verdict + relationship + links_files;
    let rss_growth = match (rss_before, rss_after) {
        (Some(b), Some(a)) => Some(a.saturating_sub(b)),
        _ => None,
    };
    Ok(serde_json::json!({
        "messages": n,
        // every link host the messages carry, and the rows kept for them
        "link_hosts": links,
        "link_rows": link_rows,
        "file_rows": files,
        "keys_without_links_and_files": keys_one,
        // what the store counts as its own, per family
        "kevy_bytes": {
            "messages": msg,
            "verdicts": verdict,
            "relationships": relationship,
            "links_and_files": links_files,
            "total": total,
        },
        "kevy_bytes_per_10k_messages": {
            "messages": per_10k(msg, n),
            "verdicts": per_10k(verdict, n),
            "relationships": per_10k(relationship, n),
            "links_and_files": per_10k(links_files, n),
            "total": per_10k(total, n),
        },
        // the process's growth across store one's writes: a second reading
        // that fails differently from the store's own count
        "rss_growth_writes_only": rss_growth,
        "rss_growth_writes_only_per_10k": rss_growth.and_then(|g| per_10k(g, n)),
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

    /// The families are measured on the store's own count, and each one
    /// written is non-zero: a zero would read as "costs nothing".
    #[test]
    fn each_family_written_is_counted() {
        let entries: Vec<Entry> = (0..5)
            .map(|i| Entry {
                facts: mailrs_mailbox_kevy::ledger::LedgerFacts {
                    message_id: format!("m{i}@x"),
                    user: "u@x".into(),
                    from_addr: format!("a{i}@sender.example"),
                    from_domain: "sender.example".into(),
                    reply_to: String::new(),
                    link_hosts: vec!["pay.example".into(), "cdn.example".into()],
                    attachment_sha256: vec![format!("{i:064x}")],
                    auth: "pass/pass/pass".into(),
                    arrived: chrono::Utc::now().timestamp() - i,
                    from_reg: "sender.example".into(),
                },
                verdict: mailrs_mailbox_kevy::ledger::LedgerVerdict {
                    rules: vec!["rule_a".into()],
                    score: 1.0,
                    rules_version: "v1".into(),
                    judged_at: 1_700_000_000,
                    ..Default::default()
                },
                name: format!("Sender {i}"),
            })
            .collect();
        let out = measure(&entries).unwrap();
        assert_eq!(out["messages"], 5, "{out}");
        assert_eq!(out["link_rows"], 10, "{out}");
        for family in [
            "messages",
            "verdicts",
            "relationships",
            "links_and_files",
            "total",
        ] {
            let bytes = out["kevy_bytes"][family].as_u64().unwrap();
            assert!(bytes > 0, "{family}: {out}");
        }
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
