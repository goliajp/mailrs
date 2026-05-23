//! [`InMemoryRateLimitStore`] — DashMap-backed [`RateLimitStore`].
//!
//! Internally encodes per-key state as a single `AtomicU64` storing the
//! theoretical arrival time (GCRA-style; see comment on [`InMemoryRateLimitStore`]).
//! The pure-math primitive at [`crate::token_bucket::evaluate_bucket`] is kept
//! as a separate exported function for callers who want explicit state.
//!
//! `check` cost: one [`dashmap::DashMap::get`] (shard *read* lock — cheap,
//! parallel) + one `AtomicU64::compare_exchange_weak` loop iteration on the
//! warm path. **No write locks, no allocations.**

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use dashmap::DashMap;
use quanta::{Clock, Instant};

use crate::config::TokenBucketConfig;
use crate::store::RateLimitStore;

/// In-process token-bucket store.
///
/// **Storage trick (borrowed from `governor`):** instead of the obvious
/// `DashMap<String, Bucket { tokens: f64, last_refill: u64 }>` (16-byte
/// state, written under a DashMap *write* lock), we store a single
/// `AtomicU64` per key holding the *theoretical arrival time* (TAT) in
/// nanoseconds since the Unix epoch. GCRA's TAT encodes the same
/// information as `(tokens, last_refill)` for a uniform-rate bucket — but
/// fits in 8 bytes, so updates are a lock-free `compare_exchange_weak`
/// instead of a write-locked mutation.
///
/// Concretely: token-bucket semantics with `capacity = N`, `refill_rate = R`
/// per second are equivalent to GCRA's `(emission_interval = 1/R seconds,
/// burst_window = N/R seconds)`. We precompute both at construction time
/// in nanoseconds.
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
    /// Per-key TAT (theoretical arrival time, monotonic nanos since
    /// [`Self::start`]). Read path takes only the DashMap shard's *read*
    /// lock; the AtomicU64 CAS happens under that read lock, so multiple
    /// keys (and even multiple checks on the same key) can proceed in
    /// parallel.
    buckets: DashMap<String, AtomicU64>,
    /// Emission interval per token, in nanoseconds. Precomputed from
    /// `1e9 / refill_rate` so the hot path doesn't divide.
    nanos_per_token: u64,
    /// Burst window: how far in the future the TAT may run ahead of `now`
    /// before we deny. Equals `capacity * nanos_per_token`.
    burst_nanos: u64,
    /// `quanta::Clock` — same monotonic clock `governor` uses for its
    /// own hot path. Returns u64-backed `Instant`s directly without going
    /// through `std::time::Duration` (saves ~3-5 ns/call). The first
    /// `Clock::new()` does a brief one-time calibration; subsequent
    /// `now()` calls are sub-10 ns.
    clock: Clock,
    /// Monotonic-clock anchor. Subtract `clock.now() - start` to get the
    /// quanta-`Duration` since store creation; `as_nanos() as u64` then
    /// gives us the TAT-space time without the u128 detour.
    start: Instant,
    /// Wall-clock anchor for `cleanup_stale_sync` so that callers can
    /// keep passing `before_unix_secs` and have it mean what they think.
    /// Converted to monotonic-since-start at call time.
    start_unix_nanos: u64,
}

impl InMemoryRateLimitStore {
    /// Construct a new store with the given config.
    ///
    /// The same config applies to every key. Use multiple
    /// `InMemoryRateLimitStore` instances if you need per-tier limits
    /// (e.g. one for auth endpoints at 10/min, one for general API at
    /// 300/min).
    pub fn new(config: TokenBucketConfig) -> Self {
        let nanos_per_token = if config.refill_rate > 0.0 {
            (1_000_000_000.0_f64 / config.refill_rate) as u64
        } else {
            // refill_rate = 0 → tokens never replenish in practice. Use a
            // large-but-finite emission interval (1 day) so the burst
            // arithmetic stays within u64 *and* no realistic workload ever
            // accidentally sees a refill. With capacity = 100 and emission
            // = 1 day, `burst_nanos = 100 days` — comfortably finite, and
            // even at 1 µs per check no workload comes near the refill.
            86_400 * 1_000_000_000
        };
        let burst_nanos = u64::from(config.capacity).saturating_mul(nanos_per_token);
        let clock = Clock::new();
        let start = clock.now();
        let start_unix_nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
            .unwrap_or(0);
        Self {
            config,
            buckets: DashMap::new(),
            nanos_per_token,
            burst_nanos,
            clock,
            start,
            start_unix_nanos,
        }
    }

    /// Synchronous (non-async) check variant.
    ///
    /// Identical semantics to [`RateLimitStore::check`], without the
    /// `async fn`'s boxed-future overhead.
    #[inline]
    pub fn check_sync(&self, key: &str) -> bool {
        // quanta::Clock + quanta::Instant — same fast monotonic clock
        // governor uses (~3-5 ns/call on Mac / Linux). The
        // `duration_since` step returns a quanta `Duration` (u64-backed);
        // `.as_nanos() as u64` is a no-op cast on 64-bit platforms.
        let now = (self.clock.now() - self.start).as_nanos() as u64;
        self.check_at_nanos(key, now)
    }

    /// Synchronous cleanup variant.
    ///
    /// Removes keys whose *last observed activity* is strictly before the
    /// supplied cutoff (in unix seconds). Activity is approximated as
    /// `TAT - emission_interval` — i.e. "when was the most recent
    /// successful check?" This matches the original `last_refill_unix_secs`
    /// semantics that callers relied on for the (rare) cleanup pass.
    /// A bucket whose last activity equals the cutoff is retained.
    pub fn cleanup_stale_sync(&self, before_unix_secs: u64) {
        // Convert the public unix-secs cutoff into our internal monotonic-
        // since-start nanos space. Both clocks advance at the same rate
        // (modulo NTP slew, which we accept — cleanup is approximate
        // anyway and the alternative is a per-bucket SystemTime read).
        let cutoff_unix_nanos = before_unix_secs.saturating_mul(1_000_000_000);
        let cutoff_mono = cutoff_unix_nanos.saturating_sub(self.start_unix_nanos);
        let emission = self.nanos_per_token;
        self.buckets.retain(|_, atomic_tat| {
            let tat = atomic_tat.load(Ordering::Relaxed);
            // Last activity ≈ TAT - emission (one emission was added by
            // the last allowed check). Buckets older than the cutoff get
            // evicted. `>=` so the boundary case is retained.
            tat.saturating_sub(emission) >= cutoff_mono
        });
    }

    /// Synchronous len.
    pub fn len_sync(&self) -> usize {
        self.buckets.len()
    }

    /// True iff no keys are tracked.
    pub fn is_empty(&self) -> bool {
        self.buckets.is_empty()
    }

    /// Testable variant of `check_sync` taking an explicit `now` in
    /// fake seconds (the test's own time space, multiplied to nanos).
    /// Bypasses the [`Instant`]-based monotonic clock so tests can
    /// inject deterministic time values.
    #[cfg(test)]
    fn check_at(&self, key: &str, now_secs: u64) -> bool {
        self.check_at_nanos(key, now_secs.saturating_mul(1_000_000_000))
    }

    /// Test-only cleanup using the same fake-seconds convention as
    /// [`Self::check_at`]. The production [`Self::cleanup_stale_sync`]
    /// takes real unix seconds and translates to monotonic via the
    /// wall-clock anchor — that path is correct for live workloads but
    /// invalid for tests injecting low fake values.
    #[cfg(test)]
    fn cleanup_at(&self, before_secs: u64) {
        let cutoff = before_secs.saturating_mul(1_000_000_000);
        let emission = self.nanos_per_token;
        self.buckets.retain(|_, atomic_tat| {
            let tat = atomic_tat.load(Ordering::Relaxed);
            tat.saturating_sub(emission) >= cutoff
        });
    }

    /// Core check, parameterised by `now` in nanoseconds since the Unix
    /// epoch. The hot path is:
    ///
    /// 1. `buckets.get(key)` — DashMap read-shared lock on the shard.
    /// 2. `AtomicU64::compare_exchange_weak` loop — lock-free.
    ///
    /// No allocations on the warm path.
    #[inline]
    fn check_at_nanos(&self, key: &str, now_nanos: u64) -> bool {
        // Fast path: existing key. `DashMap::get(&str)` takes a read-shared
        // lock on the shard (cheap, parallel-safe). The actual TAT update
        // happens via `AtomicU64::compare_exchange_weak` underneath.
        if let Some(atomic_tat) = self.buckets.get(key) {
            return self.try_acquire(&atomic_tat, now_nanos);
        }
        // Slow path: first touch. One `to_owned()` allocation + DashMap
        // entry insert (write lock on the shard). Race-safe: if another
        // thread beat us to the insert, we fall through to the same CAS
        // loop on whichever atomic ended up in the map.
        let entry = self.buckets.entry(key.to_owned()).or_insert_with(|| {
            // Initialise the TAT one emission-interval past `now`, which
            // accounts for the very request that's triggering the insert.
            // Net effect: first request always succeeds and consumes one
            // token's worth of burst.
            AtomicU64::new(now_nanos.saturating_add(self.nanos_per_token))
        });
        // If we won the race (Vacant → Occupied transition above) the
        // value was just initialised with +1 emission and the request
        // counts as allowed. If we lost the race (someone else inserted),
        // we still need to acquire — run the CAS path.
        let raw_tat = entry.value().load(Ordering::Acquire);
        let initial_tat = now_nanos.saturating_add(self.nanos_per_token);
        if raw_tat == initial_tat {
            // We won the insert race — already counted.
            true
        } else {
            self.try_acquire(entry.value(), now_nanos)
        }
    }

    /// GCRA core. Loops on `compare_exchange_weak` until the TAT update
    /// commits or the burst budget rejects the request.
    #[inline]
    fn try_acquire(&self, atomic_tat: &AtomicU64, now_nanos: u64) -> bool {
        let mut current = atomic_tat.load(Ordering::Acquire);
        loop {
            // GCRA: theoretical arrival time advances by emission interval
            // per successful check; clamped to `now` so an idle bucket
            // doesn't accumulate beyond the burst window.
            let next_tat = current.max(now_nanos).saturating_add(self.nanos_per_token);
            // Deny iff the new TAT runs more than `burst_nanos` past now.
            if next_tat.saturating_sub(now_nanos) > self.burst_nanos {
                return false;
            }
            match atomic_tat.compare_exchange_weak(
                current,
                next_tat,
                Ordering::Release,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(actual) => current = actual,
            }
        }
    }

    /// Read-only accessor for the config (used by tests/diagnostics).
    pub fn config(&self) -> &TokenBucketConfig {
        &self.config
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

        store.cleanup_at(101);
        assert_eq!(store.len_sync(), 0);
    }

    #[test]
    fn cleanup_preserves_fresh_entries() {
        let store = InMemoryRateLimitStore::new(TokenBucketConfig::default());
        store.check_at("old", 100);
        store.check_at("new", 105);
        assert_eq!(store.len_sync(), 2);

        // cutoff between them — only "old" is stale
        store.cleanup_at(103);
        assert_eq!(store.len_sync(), 1);
    }

    #[test]
    fn cleanup_boundary_exact_timestamp_retained() {
        // Entry whose TAT is exactly at the cutoff should be retained
        // (>= comparison). With the GCRA encoding, the TAT after the
        // first check at t=100 is 100 + emission_interval; cleanup with
        // cutoff=100 must therefore retain it.
        let store = InMemoryRateLimitStore::new(TokenBucketConfig::default());
        store.check_at("k", 100);
        store.cleanup_at(100);
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

        // Cleanup with cutoff well past any TAT this bucket could have.
        store.cleanup_at(u64::MAX / 2);
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
