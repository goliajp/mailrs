//! [`InMemoryRateLimitStore`] — DashMap-backed [`RateLimitStore`].
//!
//! Lock-free per-key bucket storage suitable for single-process
//! deployments. `check` is sub-microsecond on a modern x86 / ARM
//! laptop (see `tests/perf_gate.rs`). For distributed deployments,
//! implement [`RateLimitStore`] over your shared cache.

use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use dashmap::DashMap;

use crate::config::TokenBucketConfig;
use crate::store::RateLimitStore;
use crate::token_bucket::{Bucket, evaluate_bucket};

/// In-process token-bucket store.
///
/// One [`DashMap`] entry per key, keyed by owned `String`. The map is
/// not bounded — call [`RateLimitStore::cleanup_stale`] periodically
/// to evict idle keys.
///
/// `check` cost on a fresh key is one allocation (the `String` clone)
/// plus a DashMap insert. On a hot key it is one DashMap entry-lock +
/// the pure-math step. Sub-µs median in practice.
///
/// # Examples
///
/// ```
/// use mailrs_rate_limit::{InMemoryRateLimitStore, RateLimitStore, TokenBucketConfig};
///
/// # async fn demo() {
/// let store = InMemoryRateLimitStore::new(TokenBucketConfig {
///     capacity: 5,
///     refill_rate: 1.0,
/// });
///
/// for _ in 0..5 {
///     assert!(store.check("192.0.2.1").await);
/// }
/// assert!(!store.check("192.0.2.1").await); // 6th rejected
/// # }
/// ```
pub struct InMemoryRateLimitStore {
    config: TokenBucketConfig,
    buckets: DashMap<String, Bucket>,
}

impl InMemoryRateLimitStore {
    /// Construct a new store with the given config.
    ///
    /// The same config applies to every key. Use multiple
    /// `InMemoryRateLimitStore` instances if you need per-tier limits
    /// (e.g. one for auth endpoints at 10/min, one for general API at
    /// 300/min).
    pub fn new(config: TokenBucketConfig) -> Self {
        Self {
            config,
            buckets: DashMap::new(),
        }
    }

    /// Synchronous (non-async) check variant.
    ///
    /// Identical semantics to [`RateLimitStore::check`], without the
    /// `async fn`'s boxed-future overhead. Useful for hot sync paths
    /// where the caller knows they're not on an executor thread.
    pub fn check_sync(&self, key: &str) -> bool {
        let now = now_unix_secs();
        self.check_at(key, now)
    }

    /// Synchronous cleanup variant.
    pub fn cleanup_stale_sync(&self, before_unix_secs: u64) {
        self.buckets
            .retain(|_, bucket| bucket.last_refill_unix_secs >= before_unix_secs);
    }

    /// Synchronous len.
    pub fn len_sync(&self) -> usize {
        self.buckets.len()
    }

    /// True iff no keys are tracked.
    pub fn is_empty(&self) -> bool {
        self.buckets.is_empty()
    }

    /// Testable variant of `check_sync` taking an explicit `now`
    /// (unix seconds) — avoids the `SystemTime::now()` syscall.
    fn check_at(&self, key: &str, now_unix_secs: u64) -> bool {
        // Fast path: existing bucket. `DashMap::get_mut` takes `&str` via
        // the `Q: Borrow<K>` bound — no `key.to_owned()` alloc on the hot
        // path. The `entry(...)` alternative always owns the key, costing
        // one String allocation per check even when the key already exists.
        if let Some(mut entry) = self.buckets.get_mut(key) {
            let (next_state, allowed) =
                evaluate_bucket(*entry.value(), now_unix_secs, &self.config);
            *entry.value_mut() = next_state;
            return allowed;
        }
        // Slow path: insert with owned key. Pays the alloc only on first
        // touch of a never-before-seen key.
        let mut entry = self.buckets.entry(key.to_owned()).or_insert_with(|| Bucket {
            tokens: f64::from(self.config.capacity),
            last_refill_unix_secs: now_unix_secs,
        });
        let (next_state, allowed) = evaluate_bucket(*entry.value(), now_unix_secs, &self.config);
        *entry.value_mut() = next_state;
        allowed
    }
}

#[async_trait]
impl RateLimitStore for InMemoryRateLimitStore {
    async fn check(&self, key: &str) -> bool {
        self.check_sync(key)
    }

    async fn cleanup_stale(&self, before_unix_secs: u64) {
        self.cleanup_stale_sync(before_unix_secs);
    }

    async fn len(&self) -> usize {
        self.len_sync()
    }
}

/// Current unix-seconds wall clock.
///
/// Falls back to 0 on the (impossible-on-a-sane-host) case of the
/// system clock being before the Unix epoch.
fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn allow_within_capacity() {
        let store = InMemoryRateLimitStore::new(TokenBucketConfig {
            capacity: 3,
            refill_rate: 0.0,
        });
        assert!(store.check_sync("k"));
        assert!(store.check_sync("k"));
        assert!(store.check_sync("k"));
    }

    #[test]
    fn reject_over_capacity() {
        let store = InMemoryRateLimitStore::new(TokenBucketConfig {
            capacity: 2,
            refill_rate: 0.0,
        });
        assert!(store.check_sync("k"));
        assert!(store.check_sync("k"));
        assert!(!store.check_sync("k"));
    }

    #[test]
    fn refill_over_time() {
        let store = InMemoryRateLimitStore::new(TokenBucketConfig {
            capacity: 1,
            refill_rate: 1.0,
        });
        assert!(store.check_at("k", 100));
        assert!(!store.check_at("k", 100));
        // 1 sec later
        assert!(store.check_at("k", 101));
        assert!(!store.check_at("k", 101));
    }

    #[test]
    fn cleanup_removes_stale() {
        let store = InMemoryRateLimitStore::new(TokenBucketConfig::default());
        store.check_at("k", 100);
        assert_eq!(store.len_sync(), 1);

        store.cleanup_stale_sync(101);
        assert_eq!(store.len_sync(), 0);
    }

    #[test]
    fn cleanup_preserves_fresh_entries() {
        let store = InMemoryRateLimitStore::new(TokenBucketConfig::default());
        store.check_at("old", 100);
        store.check_at("new", 105);
        assert_eq!(store.len_sync(), 2);

        // cutoff between them — only "old" is stale
        store.cleanup_stale_sync(103);
        assert_eq!(store.len_sync(), 1);
    }

    #[test]
    fn cleanup_boundary_exact_timestamp_retained() {
        // entry at exactly the cutoff should be retained (>= comparison)
        let store = InMemoryRateLimitStore::new(TokenBucketConfig::default());
        store.check_at("k", 100);
        store.cleanup_stale_sync(100);
        assert_eq!(store.len_sync(), 1);
    }

    #[test]
    fn per_key_isolation() {
        let store = InMemoryRateLimitStore::new(TokenBucketConfig {
            capacity: 1,
            refill_rate: 0.0,
        });
        assert!(store.check_at("a", 100));
        assert!(!store.check_at("a", 100));
        // "b" still has its own bucket
        assert!(store.check_at("b", 100));
        assert!(!store.check_at("b", 100));
    }

    #[test]
    fn many_keys_tracked_independently() {
        let store = InMemoryRateLimitStore::new(TokenBucketConfig {
            capacity: 1,
            refill_rate: 0.0,
        });

        for i in 0..100 {
            let key = format!("k{i}");
            assert!(store.check_at(&key, 100));
        }
        assert_eq!(store.len_sync(), 100);

        for i in 0..100 {
            let key = format!("k{i}");
            assert!(!store.check_at(&key, 100));
        }
    }

    #[test]
    fn fresh_store_is_empty() {
        let store = InMemoryRateLimitStore::new(TokenBucketConfig::default());
        assert!(store.is_empty());
        assert_eq!(store.len_sync(), 0);
    }

    #[test]
    fn first_check_for_new_key_always_allowed() {
        let store = InMemoryRateLimitStore::new(TokenBucketConfig {
            capacity: 1,
            refill_rate: 0.0,
        });
        for i in 0..256 {
            let key = format!("ip-{i}");
            assert!(
                store.check_sync(&key),
                "first check for new key should always succeed"
            );
        }
    }

    #[test]
    fn check_after_cleanup_creates_fresh_bucket() {
        let store = InMemoryRateLimitStore::new(TokenBucketConfig {
            capacity: 2,
            refill_rate: 0.0,
        });

        store.check_at("k", 100);
        store.check_at("k", 100);
        assert!(!store.check_at("k", 100));

        store.cleanup_stale_sync(101);
        assert!(store.is_empty());

        // fresh bucket at full capacity
        assert!(store.check_at("k", 200));
        assert!(store.check_at("k", 200));
        assert!(!store.check_at("k", 200));
    }

    #[tokio::test]
    async fn async_check_matches_sync_check() {
        let store = InMemoryRateLimitStore::new(TokenBucketConfig {
            capacity: 1,
            refill_rate: 0.0,
        });
        assert!(store.check("k").await);
        assert!(!store.check("k").await);
        // async len matches sync
        assert_eq!(store.len().await, 1);
    }

    #[tokio::test]
    async fn async_cleanup_matches_sync_cleanup() {
        let store = InMemoryRateLimitStore::new(TokenBucketConfig::default());
        store.check("k").await;
        assert_eq!(store.len().await, 1);

        // far-future cutoff drops everything
        store.cleanup_stale(u64::MAX).await;
        assert_eq!(store.len().await, 0);
    }

    #[test]
    fn concurrent_checks_from_same_key() {
        let store = Arc::new(InMemoryRateLimitStore::new(TokenBucketConfig {
            capacity: 100,
            refill_rate: 0.0,
        }));

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let store = Arc::clone(&store);
                std::thread::spawn(move || {
                    let mut allowed = 0u32;
                    for _ in 0..20 {
                        if store.check_sync("k") {
                            allowed += 1;
                        }
                    }
                    allowed
                })
            })
            .collect();

        let total_allowed: u32 = handles.into_iter().map(|h| h.join().unwrap()).sum();
        assert_eq!(total_allowed, 100, "total allowed should equal capacity");
    }
}
