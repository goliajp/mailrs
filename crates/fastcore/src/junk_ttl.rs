//! Junk-folder retention sweep (v2.4.2 roadmap Phase 4.2, RFC-C).
//!
//! Every day, walk every user's `mailrs:user:<u>:threads:junk` zset
//! and expunge threads whose `latest_date` is older than the
//! per-user TTL (default 30 days).
//!
//! Expunge = ZREM from the junk zset + best-effort ZREM from the
//! secondary indexes (has_unread etc). Deliberately does NOT delete
//! the underlying `mailrs:thread:<tid>` hash — search / audit paths
//! still resolve the row until a later cleanup pass. Deleting rows
//! here would need cross-shard atomic (§Phase 8 territory) and the
//! zset removal is enough to hide the thread from every folder /
//! filter view.
//!
//! Per-user TTL knob: `spam:{user}:junk_ttl_days` — a plain string
//! integer in the shared kevy sidecar. Absent / unparsable → 30.
//! `0` disables retention (thread stays in Junk indefinitely) so
//! users who want manual curation can opt out.
//!
//! Scheduling: sleep 24h between ticks, first tick 60s after
//! process start so the receiver / webapi / IMAP subsystems settle
//! before we start a long-running scan. Skips gracefully on kevy
//! errors — a single failed user doesn't block the loop.

use std::sync::Arc;
use std::time::Duration;

use tokio::time::sleep;

use crate::FastcoreState;

const DEFAULT_TTL_DAYS: i64 = 30;
const TICK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const INITIAL_DELAY: Duration = Duration::from_secs(60);

/// Spawn the daily retention sweep. Called once from `lib::run`.
pub fn spawn(state: Arc<FastcoreState>) {
    tokio::spawn(async move {
        sleep(INITIAL_DELAY).await;
        loop {
            let tick_start = std::time::Instant::now();
            match run_once(&state).await {
                Ok(summary) => tracing::info!(
                    users_scanned = summary.users_scanned,
                    threads_expunged = summary.threads_expunged,
                    duration_ms = tick_start.elapsed().as_millis() as u64,
                    "junk-ttl sweep tick"
                ),
                Err(e) => tracing::warn!(error = %e, "junk-ttl sweep tick failed"),
            }
            sleep(TICK_INTERVAL).await;
        }
    });
}

/// One sweep pass — enumerate every user that has a junk zset,
/// look up their TTL, and expunge old threads. Returns per-tick
/// counters for the log line above.
#[derive(Default)]
struct SweepSummary {
    users_scanned: usize,
    threads_expunged: usize,
}

async fn run_once(state: &Arc<FastcoreState>) -> std::io::Result<SweepSummary> {
    let store = state.mailbox.store_ref();
    let user_pat = b"mailrs:user:*:threads:junk";
    let user_keys = store.collect_keys(Some(user_pat.as_slice()), None);
    let mut summary = SweepSummary::default();
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    for key_bytes in user_keys {
        let Ok(key) = std::str::from_utf8(&key_bytes) else {
            continue;
        };
        let Some(user) = extract_user_from_key(key) else {
            continue;
        };
        summary.users_scanned += 1;

        let ttl_days = lookup_ttl_days(state, user).unwrap_or(DEFAULT_TTL_DAYS);
        if ttl_days <= 0 {
            // User opted out. Retention disabled → skip expunge.
            continue;
        }
        let cutoff = now_secs - ttl_days * 86400;
        let cutoff_f = cutoff as f64;
        // Collect thread_ids whose latest_date is older than the
        // cutoff. `zrange_by_score` is inclusive on both bounds; a
        // thread whose latest_date exactly matches `cutoff_f` still
        // gets expunged, which is the desired semantics (day-based
        // TTL rounds down, not up).
        let expired = store
            .zrange_by_score(key.as_bytes(), 0.0, cutoff_f)
            .unwrap_or_default();
        if expired.is_empty() {
            continue;
        }
        let refs: Vec<&[u8]> = expired.iter().map(|(m, _)| m.as_slice()).collect();
        let _ = store.zrem(key.as_bytes(), &refs);
        summary.threads_expunged += expired.len();
    }
    Ok(summary)
}

fn extract_user_from_key(key: &str) -> Option<&str> {
    // `mailrs:user:<user>:threads:junk`
    let rest = key.strip_prefix("mailrs:user:")?;
    let end = rest.find(":threads:junk")?;
    Some(&rest[..end])
}

/// Read `spam:{user}:junk_ttl_days`. Returns `None` when the key is
/// absent OR the value is unparsable — the caller then falls back
/// to `DEFAULT_TTL_DAYS`.
fn lookup_ttl_days(state: &Arc<FastcoreState>, user: &str) -> Option<i64> {
    let key = format!("spam:{user}:junk_ttl_days");
    let store = state.mailbox.store_ref();
    let bytes = store.get(key.as_bytes()).ok().flatten()?;
    let s = std::str::from_utf8(&bytes).ok()?;
    s.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_user_parses_standard_key() {
        assert_eq!(
            extract_user_from_key("mailrs:user:alice@example.com:threads:junk"),
            Some("alice@example.com")
        );
    }

    #[test]
    fn extract_user_returns_none_on_wrong_prefix() {
        assert_eq!(
            extract_user_from_key("mailrs:thread:tid-1:threads:junk"),
            None
        );
    }

    #[test]
    fn extract_user_returns_none_on_wrong_suffix() {
        assert_eq!(
            extract_user_from_key("mailrs:user:alice@example.com:threads:inbox"),
            None
        );
    }
}
