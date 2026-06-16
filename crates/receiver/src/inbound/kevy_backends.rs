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

use std::io;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use kevy_client::Connection;

use mailrs_shield::greylist::GreylistBackend;

use crate::inbound::auth_guard::{
    AuthCheck, AuthGuardConfig, AuthGuardStore, normalize_ip, unix_now,
};
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

/// Increment a scope's failure counter; arm a lockout when it reaches
/// `max`. The counter self-expires after `window`; on lockout the
/// counter is reset (during a lockout the session rejects before
/// verifying, so no failures accumulate). One blocking round-trip
/// sequence on a single connection.
fn bump_failure_scope(
    c: &mut Connection,
    fail_key: &[u8],
    lock_key: &[u8],
    max: i64,
    window: Duration,
    lockout: Duration,
) -> io::Result<()> {
    let count = c.incr(fail_key)?;
    if count == 1 {
        c.expire(fail_key, window)?;
    }
    if count >= max {
        c.set_with_ttl(lock_key, b"1", lockout)?;
        c.del(&[fail_key])?;
    }
    Ok(())
}

/// Network [`AuthGuardStore`] over a shared kevy-server — a distributed
/// brute-force lockout via `INCR` failure counters + lockout keys.
///
/// Simpler than the in-process sliding-window+exponential-backoff
/// tracker, by design (per the receiver plan): per-scope failure
/// counters with a window TTL, and on threshold a lockout key with a
/// fixed `base_lockout` TTL. It keys on the /64-normalized IP so the
/// per-IP lockout has the same evasion-resistance as the in-process
/// impl. **Difference from in-process:** repeat offenders do not get an
/// exponentially-growing lockout — every lockout is the base duration.
///
/// Fail-open: a `check` error reads as Allowed (a kevy outage must not
/// lock out legitimate users); record errors are dropped.
pub struct KevyServerAuthGuardStore {
    client: Arc<KevyNetClient>,
    max_failures_account: i64,
    account_window: Duration,
    account_lockout: Duration,
    max_failures_ip: i64,
    ip_window: Duration,
    ip_lockout: Duration,
}

impl KevyServerAuthGuardStore {
    pub fn new(client: Arc<KevyNetClient>, config: AuthGuardConfig) -> Self {
        Self {
            client,
            max_failures_account: i64::from(config.max_failures_account),
            account_window: Duration::from_secs(config.account_window_secs),
            account_lockout: Duration::from_secs(config.base_lockout_secs),
            max_failures_ip: i64::from(config.max_failures_ip),
            ip_window: Duration::from_secs(config.ip_window_secs),
            ip_lockout: Duration::from_secs(config.ip_base_lockout_secs),
        }
    }
}

#[async_trait]
impl AuthGuardStore for KevyServerAuthGuardStore {
    async fn check(&self, ip: IpAddr, username: &str, _now: u64) -> AuthCheck {
        let ip = normalize_ip(ip);
        let ip_lock = format!("ag:li:{ip}").into_bytes();
        let acct_lock = format!("ag:la:{ip}:{username}").into_bytes();
        let client = self.client.clone();
        // per-IP first, then per-account — matches in-process precedence.
        let remaining: io::Result<Option<u64>> = tokio::task::spawn_blocking(move || {
            client.with_conn(|c| {
                for key in [&ip_lock, &acct_lock] {
                    let ms = c.ttl_ms(key)?;
                    if ms > 0 {
                        return Ok(Some(((ms / 1000) as u64).max(1)));
                    }
                }
                Ok(None)
            })
        })
        .await
        .unwrap_or(Ok(None));
        // None or any error → Allowed (fail-open).
        match remaining {
            Ok(Some(secs)) => AuthCheck::LockedOut {
                remaining_secs: secs,
            },
            _ => AuthCheck::Allowed,
        }
    }

    async fn record_failure(&self, ip: IpAddr, username: &str, _now: u64) {
        let ip = normalize_ip(ip);
        let acct_fail = format!("ag:fa:{ip}:{username}").into_bytes();
        let acct_lock = format!("ag:la:{ip}:{username}").into_bytes();
        let ip_fail = format!("ag:fi:{ip}").into_bytes();
        let ip_lock = format!("ag:li:{ip}").into_bytes();
        let max_a = self.max_failures_account;
        let win_a = self.account_window;
        let lock_a = self.account_lockout;
        let max_i = self.max_failures_ip;
        let win_i = self.ip_window;
        let lock_i = self.ip_lockout;
        let client = self.client.clone();
        let _ = tokio::task::spawn_blocking(move || {
            client.with_conn(|c| {
                bump_failure_scope(c, &acct_fail, &acct_lock, max_a, win_a, lock_a)?;
                bump_failure_scope(c, &ip_fail, &ip_lock, max_i, win_i, lock_i)?;
                Ok(())
            })
        })
        .await;
    }

    async fn record_success(&self, ip: IpAddr, username: &str) {
        let ip = normalize_ip(ip);
        let acct_fail = format!("ag:fa:{ip}:{username}").into_bytes();
        let acct_lock = format!("ag:la:{ip}:{username}").into_bytes();
        let client = self.client.clone();
        // clear the per-account counter + lockout only — never the per-IP
        // scope (matches the in-process contract).
        let _ = tokio::task::spawn_blocking(move || {
            client.with_conn(|c| {
                c.del(&[acct_fail.as_slice(), acct_lock.as_slice()])?;
                Ok(())
            })
        })
        .await;
    }

    async fn cleanup_stale(&self, _before: u64) {
        // kevy expires failure counters + lockout keys natively.
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

    fn auth_config(max_account: u32) -> AuthGuardConfig {
        AuthGuardConfig {
            max_failures_account: max_account,
            account_window_secs: 900,
            base_lockout_secs: 60,
            // keep IP scope out of the way for the account-focused test
            max_failures_ip: 1000,
            ip_window_secs: 3600,
            ip_base_lockout_secs: 3600,
            backoff_multiplier: 2.0,
            max_lockout_secs: 86400,
        }
    }

    #[tokio::test]
    async fn auth_backend_locks_after_threshold_then_success_clears() {
        let client = Arc::new(KevyNetClient::new("mem://auth-backend-test"));
        let store = KevyServerAuthGuardStore::new(client, auth_config(2));
        let ip: IpAddr = "203.0.113.9".parse().unwrap();

        assert!(matches!(
            store.check(ip, "carol", 1_000).await,
            AuthCheck::Allowed
        ));
        store.record_failure(ip, "carol", 1_000).await;
        store.record_failure(ip, "carol", 1_000).await; // hits threshold → lockout
        assert!(matches!(
            store.check(ip, "carol", 1_000).await,
            AuthCheck::LockedOut { remaining_secs } if remaining_secs > 0
        ));
        // a different account on the same IP is unaffected (IP scope high)
        assert!(matches!(
            store.check(ip, "dave", 1_000).await,
            AuthCheck::Allowed
        ));
        // success clears the account lockout + counter
        store.record_success(ip, "carol").await;
        assert!(matches!(
            store.check(ip, "carol", 1_000).await,
            AuthCheck::Allowed
        ));
    }

    #[tokio::test]
    async fn auth_backend_ip_scope_locks_across_usernames() {
        // low IP threshold, high account threshold: spraying usernames
        // from one IP trips the per-IP lockout (the /64 evasion guard).
        let client = Arc::new(KevyNetClient::new("mem://auth-ip-scope-test"));
        let cfg = AuthGuardConfig {
            max_failures_account: 1000,
            max_failures_ip: 3,
            ..auth_config(1000)
        };
        let store = KevyServerAuthGuardStore::new(client, cfg);
        let ip: IpAddr = "198.51.100.7".parse().unwrap();

        store.record_failure(ip, "u1", 1_000).await;
        store.record_failure(ip, "u2", 1_000).await;
        store.record_failure(ip, "u3", 1_000).await; // 3rd → IP lockout
        assert!(matches!(
            store.check(ip, "anyone", 1_000).await,
            AuthCheck::LockedOut { .. }
        ));
    }
}
