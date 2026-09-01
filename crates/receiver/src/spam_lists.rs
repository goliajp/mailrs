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
    let bytes = client
        .with_conn(|c| c.smembers(key.as_bytes()).map_err(std::io::Error::from))
        .ok()?;
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

/// How many messages this deployment has ever had from `host`'s
/// registrable domain.
///
/// Half of the brand-impersonation check: a phish claiming to be
/// Amazon arrives from a domain nothing has ever come from, and a
/// newsletter that merely names Amazon arrives from one with a
/// history. See `mailrs_fraud::brand` for the corpus.
///
/// **Zero when the client is absent or the read fails**, which is the
/// unfamiliar answer — so a kevy outage holds a brand-claiming
/// message rather than delivering it. Every other read in this file
/// fails open; this one fails towards the hold, because the two
/// failures are not symmetric: a held message is one click from the
/// reader, a delivered phish is a credential.
pub async fn domain_seen_async(client: Option<Arc<KevyNetClient>>, host: &str) -> u64 {
    let Some(client) = client else { return 0 };
    let host_owned = host.to_string();
    tokio::task::spawn_blocking(move || {
        let key = mailrs_fraud::brand::seen_key(&mailrs_fraud::brand::registrable(&host_owned));
        client
            .with_conn(|c| c.get(key.as_bytes()).map_err(std::io::Error::from))
            .ok()
            .flatten()
            .and_then(|v| String::from_utf8_lossy(&v).trim().parse().ok())
            .unwrap_or(0)
    })
    .await
    .unwrap_or(0)
}

/// How many registrable domains send to this message's off-domain
/// `Reply-To`, counting this one.
///
/// Records first, then reads, so the domain in hand is included —
/// the fourth arrival is the one that convicts, and it should convict
/// itself rather than only its successors.
///
/// Zero without a network kevy, which reads as *not rotating*: the
/// deployment delivers rather than hides, which is the safe direction
/// to be wrong in when the store is unreachable.
pub async fn reply_rotation_async(
    client: Option<Arc<KevyNetClient>>,
    from_host: &str,
    reply_addr: &str,
) -> u32 {
    let Some(client) = client else { return 0 };
    if !mailrs_fraud::reply_rotation::is_off_domain(from_host, reply_addr) {
        return 0;
    }
    let (host, reply) = (from_host.to_string(), reply_addr.to_string());
    tokio::task::spawn_blocking(move || {
        client
            .with_conn(|c| {
                mailrs_core_sidestate::families::reply_rotation::record(c, &host, &reply);
                Ok(mailrs_core_sidestate::families::reply_rotation::domains(
                    c, &host, &reply,
                ))
            })
            .unwrap_or(0)
    })
    .await
    .unwrap_or(0)
}

/// The display names on this deployment's own accounts.
///
/// Published at boot by fastcore, which holds the account store; this
/// process does not, and needs them to see a stranger wearing one of
/// our own people's names. Empty switches that check off, which
/// delivers rather than holds.
pub async fn account_names_async(client: Option<Arc<KevyNetClient>>) -> Vec<String> {
    let Some(client) = client else {
        return Vec::new();
    };
    tokio::task::spawn_blocking(move || {
        client
            .with_conn(|c| Ok(mailrs_core_sidestate::families::account_names::read(c)))
            .unwrap_or_default()
    })
    .await
    .unwrap_or_default()
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
            .with_conn(|c| {
                c.sadd(b"spam:u@example.com:whitelist", &[b"Friend@GOLIA.jp"])
                    .map_err(std::io::Error::from)
            })
            .expect("sadd whitelist");
        client
            .with_conn(|c| {
                c.sadd(b"spam:u@example.com:blacklist", &[b"spammer@EVIL.com"])
                    .map_err(std::io::Error::from)
            })
            .expect("sadd blacklist");

        let (wl, bl) = load_recipient_lists(&client, "U@Example.com");
        assert!(wl.contains("friend@golia.jp"));
        assert!(bl.contains("spammer@evil.com"));
        assert_eq!(wl.len(), 1);
        assert_eq!(bl.len(), 1);
    }
}
