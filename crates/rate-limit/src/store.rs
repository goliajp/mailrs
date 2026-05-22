//! [`RateLimitStore`] trait — pluggable storage for token-bucket state.
//!
//! Implementations own how bucket state is stored (in-process, Redis,
//! DynamoDB, ...) and how concurrent access is serialised. The trait
//! itself is small: try-consume, cleanup, and a size hint.
//!
//! [`crate::InMemoryRateLimitStore`] is the bundled reference impl —
//! lock-free DashMap, sub-microsecond `check`. Use it when the
//! application runs in a single process; bring your own impl for
//! distributed deployments.

use async_trait::async_trait;

/// Pluggable storage trait for token-bucket rate limiting.
///
/// ## Contract
///
/// - **Per-key isolation.** Each `key` has its own independent bucket.
///   Implementations must NOT share token counts across keys.
/// - **Stateful.** A `check(k)` that returns `false` must NOT consume a
///   token (the bucket is empty by definition); a `check(k)` that
///   returns `true` consumes exactly one token.
/// - **First check on a previously-unseen key returns `true`.** Fresh
///   buckets start at full capacity, so the very first request through
///   a key (with `capacity >= 1`) is always allowed.
/// - **Sustained traffic above `refill_rate` keys/sec eventually
///   rejects.** Distributed backends may be eventually consistent —
///   the only hard guarantee is the steady-state rate.
///
/// ## Time
///
/// Implementations decide what clock to use internally; the trait
/// surface uses unix-seconds (`u64`) for the cleanup boundary because
/// that is portable across processes and clocks. In-process backends
/// can convert from `Instant` at the boundary.
///
/// ## Async
///
/// All methods are async to accommodate networked backends (Redis,
/// memcached, DynamoDB) that perform I/O on every call. Pure
/// in-process implementations may complete synchronously and return
/// immediately — `async_trait` adds only the boxed-future overhead.
///
/// ## Example
///
/// ```
/// use mailrs_rate_limit::{InMemoryRateLimitStore, RateLimitStore, TokenBucketConfig};
///
/// # async fn demo() {
/// let store = InMemoryRateLimitStore::new(TokenBucketConfig {
///     capacity: 2,
///     refill_rate: 0.0,
/// });
///
/// assert!(store.check("client-1").await);
/// assert!(store.check("client-1").await);
/// assert!(!store.check("client-1").await); // drained
///
/// // Other keys are unaffected.
/// assert!(store.check("client-2").await);
/// # }
/// ```
#[async_trait]
pub trait RateLimitStore: Send + Sync {
    /// Try to consume one token for `key`. Returns `true` if allowed.
    ///
    /// Returning `false` does NOT consume a token; the caller may
    /// re-issue the request after waiting (typically `1 / refill_rate`
    /// seconds for one fresh token).
    async fn check(&self, key: &str) -> bool;

    /// Remove buckets that haven't been touched since `before_unix_secs`.
    ///
    /// Stale-bucket cleanup is the caller's responsibility — call this
    /// periodically (e.g. once an hour from a background task) to
    /// bound memory growth. Implementations make no scheduling
    /// guarantees; some may make this a no-op if their storage layer
    /// expires keys natively (Redis TTL, DynamoDB TTL).
    async fn cleanup_stale(&self, before_unix_secs: u64);

    /// Approximate number of tracked keys.
    ///
    /// Useful for metrics dashboards. Distributed backends may return
    /// an estimate or a best-effort count from local cache. Returns 0
    /// for stores that don't track size.
    async fn len(&self) -> usize;

    /// True iff no keys are currently tracked. Default impl calls
    /// [`Self::len`]; implementations with a cheaper "is anything
    /// there?" check (e.g. a Redis EXISTS on a sentinel key) should
    /// override.
    async fn is_empty(&self) -> bool {
        self.len().await == 0
    }
}
