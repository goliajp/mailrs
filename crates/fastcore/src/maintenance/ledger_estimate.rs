//! How much memory the fact ledger would take, measured on real mail.
//!
//! Read-only on the store: the newest message of up to `limit`
//! conversations is read, and the entries the ledger design gives one
//! message are written into a fresh in-memory store that is dropped
//! afterwards. The answer is the growth of this process's resident memory
//! across the writes, and the raw bytes written, for comparison.
//!
//! Entries per message, as the sub-RFC lays them out:
//!
//! ```text
//! L:m:<message id>          hash  user, from, from domain, reply-to, link
//!                                 hosts, attachment hashes, auth, arrived,
//!                                 verdict (rules, score, rule version)
//! L:d:<from domain>         zset  message id → arrived
//! L:s:<from address>        zset  message id → arrived
//! L:l:<link host>           zset  message id → arrived, per host
//! L:h:<attachment sha256>   zset  message id → arrived, per file
//! L:r:<user>:<from address> hash  first_at, inbound (counted), name
//! ```

use super::fraud_rescan::reading::newest_raw;
use super::prelude::*;

#[derive(Debug, serde::Deserialize)]
pub(crate) struct EstimateQuery {
    #[serde(default = "default_limit")]
    limit: u64,
}

fn default_limit() -> u64 {
    10_000
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
    match tokio::task::spawn_blocking(move || estimate(&state, q.limit)).await {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Default)]
struct Tally {
    messages: u64,
    raw_bytes: u64,
    keys: std::collections::HashSet<String>,
}

impl Tally {
    fn hset(
        &mut self,
        s: &kevy_embedded::Store,
        key: &str,
        pairs: &[(&str, String)],
    ) -> Result<(), String> {
        let p: Vec<(&[u8], &[u8])> = pairs
            .iter()
            .map(|(f, v)| (f.as_bytes(), v.as_bytes()))
            .collect();
        self.raw_bytes +=
            (key.len() + pairs.iter().map(|(f, v)| f.len() + v.len()).sum::<usize>()) as u64;
        self.keys.insert(key.to_string());
        s.hset(key.as_bytes(), &p)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn zadd(
        &mut self,
        s: &kevy_embedded::Store,
        key: &str,
        member: &str,
        score: f64,
    ) -> Result<(), String> {
        self.raw_bytes += (key.len() + member.len() + 8) as u64;
        self.keys.insert(key.to_string());
        s.zadd(key.as_bytes(), &[(score, member.as_bytes())])
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

fn estimate(state: &Arc<FastcoreState>, limit: u64) -> Result<serde_json::Value, String> {
    let targets = super::fraud_rescan::snapshot(state).map_err(|e| e.to_string())?;
    let mut fraud = super::fraud_rescan::FraudJudge::new(state);
    let before = rss();
    let store =
        kevy_embedded::Store::open(kevy_embedded::Config::default()).map_err(|e| e.to_string())?;
    let mut t = Tally::default();
    let mut users = std::collections::HashSet::new();
    for (user, tid) in &targets {
        if t.messages >= limit {
            break;
        }
        let Some((mid, raw)) = newest_raw(state, user, tid) else {
            continue;
        };
        t.messages += 1;
        users.insert(user.clone());
        let parsed = mailrs_mime::parse(&raw);
        let from = mailrs_inbound::identity::from_address(&raw).to_ascii_lowercase();
        let domain = from
            .rsplit_once('@')
            .map(|(_, d)| d.to_string())
            .unwrap_or_default();
        let reply_to = mailrs_inbound::identity::reply_to_address(&raw);
        let links = mailrs_inbound::scan_input::link_hosts(&parsed);
        let hashes: Vec<String> = mailrs_inbound::scan_input::attachments(&parsed)
            .iter()
            .map(|a| {
                use sha2::Digest as _;
                sha2::Sha256::digest(a.bytes)
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect()
            })
            .collect();
        let (spf, dkim, dmarc) = mailrs_inbound::identity::auth_results_tokens(&raw);
        let at = mailrs_inbound::dates::received_epoch(&raw).unwrap_or(0);
        let verdict = match fraud.judge(state, &mid, &raw) {
            Ok(j) => serde_json::json!({
                "rules": j.findings.rules(),
                "score": j.findings.score(),
                "v": &j.version[..j.version.len().min(16)],
            })
            .to_string(),
            Err(e) => format!("error: {e}"),
        };
        t.hset(
            &store,
            &format!("L:m:{mid}"),
            &[
                ("u", user.clone()),
                ("f", from.clone()),
                ("fd", domain.clone()),
                ("rt", reply_to),
                ("ln", links.join(",")),
                ("ah", hashes.join(",")),
                ("au", format!("{spf}/{dkim}/{dmarc}")),
                ("at", at.to_string()),
                ("vd", verdict),
            ],
        )?;
        let score = at as f64;
        t.zadd(&store, &format!("L:d:{domain}"), &mid, score)?;
        t.zadd(&store, &format!("L:s:{from}"), &mid, score)?;
        for host in &links {
            t.zadd(&store, &format!("L:l:{host}"), &mid, score)?;
        }
        for h in &hashes {
            t.zadd(&store, &format!("L:h:{h}"), &mid, score)?;
        }
        let rel = format!("L:r:{user}:{from}");
        let name = mailrs_inbound::from_header(&raw);
        t.hset(
            &store,
            &rel,
            &[
                ("first_at", at.to_string()),
                ("in", "1".into()),
                ("name", name),
            ],
        )?;
    }
    let after = rss();
    drop(store);
    let dropped = rss();
    let delta = match (before, after) {
        (Some(b), Some(a)) => Some(a.saturating_sub(b)),
        _ => None,
    };
    let per_10k = delta
        .filter(|_| t.messages > 0)
        .map(|d| d * 10_000 / t.messages);
    Ok(serde_json::json!({
        "messages": t.messages,
        "users": users.len(),
        "keys": t.keys.len(),
        "raw_bytes": t.raw_bytes,
        "raw_bytes_per_message": t.raw_bytes.checked_div(t.messages),
        "rss_before": before,
        "rss_after": after,
        "rss_after_drop": dropped,
        "rss_growth": delta,
        "rss_growth_per_10k_messages": per_10k,
    }))
}
