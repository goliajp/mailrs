//! Per-user sender allow / block list snapshots for the antispam
//! pipeline (v2.4.1 roadmap Phase 3, RFC 20260711 Phase B §3.3).
//!
//! The pipeline's `PipelineInput.recipient_whitelist` /
//! `recipient_blacklist` fields are read via `SMEMBERS` on the shared
//! kevy sidecar for the primary RCPT. The reads are cheap (a set of
//! ~10-100 lowercased addresses per user) and fail open — network
//! errors return empty sets so a temporary kevy blip can't strand
//! inbound mail. Populating the pipeline's fields with empty sets
//! is the pre-Phase-3 baseline behavior, so the failure mode is
//! "no whitelist/blacklist applied to this specific message" —
//! never a bounce or drop.
//!
//! Called from `crates/receiver/src/smtp_session/events/data/antispam.rs`
//! right before `ctx.inbound_pipeline.run(&mut receive_ctx).await`.

use std::collections::HashSet;
use std::sync::Arc;

use crate::kevy_net::KevyNetClient;

/// kevy key holding the recipient's whitelist. Set of lowercased
/// email addresses. Read-only from the receiver; the webapi handles
/// writes when a user clicks "mark not junk" or manages the list
/// from settings.
fn whitelist_key(user: &str) -> String {
    format!("spam:{user}:whitelist")
}

/// Same as `whitelist_key` for the blacklist.
fn blacklist_key(user: &str) -> String {
    format!("spam:{user}:blacklist")
}

/// Snapshot both lists in one round trip pair. The caller then hands
/// the results to `ReceiveContext.recipient_whitelist` /
/// `.recipient_blacklist`.
///
/// Sync — MUST be called inside `tokio::task::spawn_blocking`. The
/// underlying `KevyNetClient::with_conn` uses a blocking socket.
pub fn load_recipient_lists(
    client: &KevyNetClient,
    user: &str,
) -> (HashSet<String>, HashSet<String>) {
    let user_lc = user.to_lowercase();
    let wl = read_lowercase_set(client, &whitelist_key(&user_lc)).unwrap_or_default();
    let bl = read_lowercase_set(client, &blacklist_key(&user_lc)).unwrap_or_default();
    (wl, bl)
}

/// Async convenience wrapper — spawns the sync helper on the blocking
/// pool so callers on the async side don't have to think about it.
/// Returns empty sets on any failure (including client absent).
pub async fn load_recipient_lists_async(
    client: Option<Arc<KevyNetClient>>,
    user: &str,
) -> (HashSet<String>, HashSet<String>) {
    let Some(client) = client else {
        return (HashSet::new(), HashSet::new());
    };
    let user_owned = user.to_string();
    tokio::task::spawn_blocking(move || load_recipient_lists(&client, &user_owned))
        .await
        .unwrap_or_default()
}

fn read_lowercase_set(client: &KevyNetClient, key: &str) -> Option<HashSet<String>> {
    let bytes = client.with_conn(|c| c.smembers(key.as_bytes())).ok()?;
    let mut out = HashSet::with_capacity(bytes.len());
    for b in bytes {
        if let Ok(s) = std::str::from_utf8(&b) {
            let t = s.trim();
            if !t.is_empty() {
                out.insert(t.to_lowercase());
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_key_returns_empty_snapshot() {
        // `mem://` URLs exercise the same command surface without a
        // TCP server — used by the KevyNetClient smoke test too.
        let client = KevyNetClient::new("mem://spam-lists-test-empty");
        let (wl, bl) = load_recipient_lists(&client, "u@example.com");
        assert!(wl.is_empty());
        assert!(bl.is_empty());
    }

    #[test]
    fn populated_key_lowercases_entries() {
        let client = KevyNetClient::new("mem://spam-lists-test-populated");
        // Seed the sets. The whitelist entry is uppercased on the way
        // in so the assertion proves normalization.
        client
            .with_conn(|c| c.sadd(b"spam:u@example.com:whitelist", &[b"Friend@GOLIA.jp"]))
            .expect("sadd whitelist");
        client
            .with_conn(|c| c.sadd(b"spam:u@example.com:blacklist", &[b"spammer@EVIL.com"]))
            .expect("sadd blacklist");

        let (wl, bl) = load_recipient_lists(&client, "U@Example.com");
        assert!(wl.contains("friend@golia.jp"));
        assert!(bl.contains("spammer@evil.com"));
        assert_eq!(wl.len(), 1);
        assert_eq!(bl.len(), 1);
    }
}
