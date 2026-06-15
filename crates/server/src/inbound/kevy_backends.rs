//! Network-kevy implementations of the anti-subsystem storage traits.
//!
//! These live in the server (cement), not in the stones, so the
//! published stones (`mailrs-shield`, `mailrs-rate-limit`,
//! `mailrs-auth-guard`) stay free of a network-client dependency. Each
//! wraps the blocking [`kevy_client::Connection`] (via
//! [`KevyNetClient`]) in `spawn_blocking` and folds errors into the
//! **fail-open** shape its trait already assumes — an unreachable
//! kevy-server must never block mail flow.
//!
//! Used only in the receiver-split topology (`MAILRS_KEVY_URL` set);
//! otherwise the subsystems run on the in-process embedded store.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use mailrs_shield::greylist::GreylistBackend;

use crate::inbound::auth_guard::unix_now;
use crate::inbound::rate_limit::{RateLimitStore, TokenBucketConfig};
use crate::kevy_net::KevyNetClient;

/// Network [`GreylistBackend`] over a shared kevy-server.
///
/// Fail-open: a read error reads as "not seen" (→ the policy greylists,
/// i.e. delays, rather than letting unverified mail straight through),
/// and writes are best-effort — exactly the shape
/// [`GreylistBackend`] documents.
pub struct KevyServerGreylistBackend {
    client: Arc<KevyNetClient>,
}

impl KevyServerGreylistBackend {
    pub fn new(client: Arc<KevyNetClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl GreylistBackend for KevyServerGreylistBackend {
    async fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        let client = self.client.clone();
        let key = key.to_vec();
        // JoinError → None, io::Error → None, miss → None: every failure
        // mode collapses to "not seen", the safe greylist default.
        tokio::task::spawn_blocking(move || client.with_conn(|c| c.get(&key)))
            .await
            .ok()
            .and_then(Result::ok)
            .flatten()
    }

    async fn set_with_ttl(&self, key: &[u8], value: &[u8], ttl: Duration) {
        let client = self.client.clone();
        let key = key.to_vec();
        let value = value.to_vec();
        let _ = tokio::task::spawn_blocking(move || {
            client.with_conn(|c| c.set_with_ttl(&key, &value, ttl))
        })
        .await;
    }

    async fn expire(&self, key: &[u8], ttl: Duration) {
        let client = self.client.clone();
        let key = key.to_vec();
        let _ = tokio::task::spawn_blocking(move || {
            client.with_conn(|c| c.expire(&key, ttl).map(|_| ()))
        })
        .await;
    }
}

/// Network [`RateLimitStore`] over a shared kevy-server — a distributed
/// fixed-window counter (`INCR` + `EXPIRE`).
///
/// The in-process store is a GCRA token bucket; a fixed window is a
/// lossy but contract-compliant approximation (the trait only promises
/// steady-state rate, and tolerates eventual consistency). The window
/// is sized so `capacity` requests are allowed per `capacity /
/// refill_rate` seconds — steady-state ≈ `refill_rate`/sec, burst ≈
/// `capacity`, matching the bucket's shape. `refill_rate <= 0` (pure
/// burst cap) maps to a 1-day window, mirroring the in-memory impl.
///
/// Fail-open: any error (unreachable server, join failure) returns
/// `true` (allow) — a kevy outage must not block mail.
pub struct KevyServerRateLimitStore {
    client: Arc<KevyNetClient>,
    limit: i64,
    window_secs: u64,
}

impl KevyServerRateLimitStore {
    pub fn new(client: Arc<KevyNetClient>, config: TokenBucketConfig) -> Self {
        let window_secs = if config.refill_rate <= 0.0 {
            86_400
        } else {
            ((f64::from(config.capacity) / config.refill_rate).round() as u64).max(1)
        };
        Self {
            client,
            limit: i64::from(config.capacity),
            window_secs,
        }
    }
}

#[async_trait]
impl RateLimitStore for KevyServerRateLimitStore {
    async fn check(&self, key: &str) -> bool {
        let client = self.client.clone();
        let window_secs = self.window_secs;
        let limit = self.limit;
        // fixed window keyed by the current bucket index; the key
        // self-expires, so distinct windows never collide.
        let bucket = unix_now() / window_secs;
        let redis_key = format!("rl:{key}:{bucket}").into_bytes();
        let ttl = Duration::from_secs(window_secs);
        tokio::task::spawn_blocking(move || {
            client.with_conn(|c| {
                let count = c.incr(&redis_key)?;
                if count == 1 {
                    // first hit in this window — arm the TTL so the
                    // counter resets when the window rolls over.
                    c.expire(&redis_key, ttl)?;
                }
                Ok(count <= limit)
            })
        })
        .await
        .unwrap_or(Ok(true)) // join failure → allow
        .unwrap_or(true) // io error → allow (fail-open)
    }

    async fn cleanup_stale(&self, _before_unix_secs: u64) {
        // kevy expires the window keys natively (EXPIRE) — nothing to do.
    }

    async fn len(&self) -> usize {
        // DBSIZE would count greylist + every other key, not just rate
        // buckets; report 0 per the trait contract for stores that don't
        // track their own size.
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Exercise the backend over `mem://` (URL-dispatched to embedded),
    // which drives the same Connection command surface the network path
    // uses — proves get/set_with_ttl round-trip through spawn_blocking.
    #[tokio::test]
    async fn greylist_backend_set_then_get_round_trip() {
        let client = Arc::new(KevyNetClient::new("mem://greylist-backend-test"));
        let backend = KevyServerGreylistBackend::new(client);

        assert_eq!(backend.get(b"gl:triplet").await, None);
        backend
            .set_with_ttl(b"gl:triplet", b"1700000000", Duration::from_secs(3600))
            .await;
        assert_eq!(
            backend.get(b"gl:triplet").await.as_deref(),
            Some(&b"1700000000"[..])
        );
    }

    #[tokio::test]
    async fn rate_backend_allows_to_capacity_then_rejects() {
        // capacity 2, refill 0 → 1-day window, limit 2. Three checks in
        // the same window: allow, allow, reject.
        let client = Arc::new(KevyNetClient::new("mem://rate-backend-test"));
        let store = KevyServerRateLimitStore::new(
            client,
            TokenBucketConfig {
                capacity: 2,
                refill_rate: 0.0,
            },
        );
        assert!(store.check("1.2.3.4").await, "1st under capacity");
        assert!(store.check("1.2.3.4").await, "2nd at capacity");
        assert!(!store.check("1.2.3.4").await, "3rd over capacity");
        // a different key has its own bucket
        assert!(store.check("5.6.7.8").await, "other key unaffected");
    }
}
