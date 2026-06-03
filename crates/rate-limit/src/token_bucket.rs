//! Pure token-bucket arithmetic.
//!
//! [`evaluate_bucket`] is the heart of the crate — it takes the state of
//! one bucket (current tokens + last-refill timestamp), the current wall
//! clock (in unix seconds), and a [`TokenBucketConfig`], and returns the
//! new bucket state plus whether the request was allowed. It performs no
//! I/O, allocates nothing, and is safe to call from a tight CAS loop.
//!
//! Backends ([`crate::InMemoryRateLimitStore`] and any custom impl) wrap
//! this with their own storage: read state → call this function → write
//! state back. The function is the contract; the rest is just storage.
//!
//! ```
//! use mailrs_rate_limit::{evaluate_bucket, Bucket, TokenBucketConfig};
//!
//! let config = TokenBucketConfig { capacity: 2, refill_rate: 1.0 };
//! let mut bucket = Bucket { tokens: 2.0, last_refill_unix_secs: 1_000 };
//!
//! // First request — bucket has tokens, allow.
//! let (next, allowed) = evaluate_bucket(bucket, 1_000, &config);
//! assert!(allowed);
//! assert!((next.tokens - 1.0).abs() < 1e-9);
//! bucket = next;
//!
//! // Second request same second — still has 1 token.
//! let (next, allowed) = evaluate_bucket(bucket, 1_000, &config);
//! assert!(allowed);
//! assert!(next.tokens < 0.001);
//! bucket = next;
//!
//! // Third request same second — empty, deny.
//! let (next, allowed) = evaluate_bucket(bucket, 1_000, &config);
//! assert!(!allowed);
//! assert!(next.tokens < 0.001);
//! ```

use crate::config::TokenBucketConfig;

/// Persistable state of one token bucket.
///
/// Backends store one `Bucket` per key. The pure
/// [`evaluate_bucket`] function takes a `Bucket` and returns a new
/// `Bucket` (immutable update). Backends are responsible for the
/// read-modify-write step against their storage.
///
/// `last_refill_unix_secs` is **unix seconds** (`u64`) — seconds since
/// `1970-01-01T00:00:00Z`. Backends that use a monotonic clock
/// internally (e.g. `Instant`) must convert at the boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bucket {
    /// Current token count. Fractional; refills accumulate continuously.
    pub tokens: f64,
    /// Wall-clock time (unix seconds) at which `tokens` was last computed.
    pub last_refill_unix_secs: u64,
}

/// Apply token-bucket arithmetic for one `check` call.
///
/// Given the bucket's current `state`, the current wall-clock `now`
/// (unix seconds), and the bucket `config`, returns the new state and
/// whether one token was consumed.
///
/// Algorithm (verbatim from mailrs's historical inbound rate limiter):
///
/// 1. Compute `elapsed = now - last_refill` (saturating at 0 — if the
///    clock went backwards, treat as no elapsed time).
/// 2. Refill: `tokens += elapsed × refill_rate`, capped at `capacity`.
/// 3. If `tokens >= 1.0`, decrement by 1 and return `allowed = true`.
///    Otherwise, leave tokens untouched and return `allowed = false`.
/// 4. Always update `last_refill` to `now`.
///
/// Backends should call this CAS-style if their storage permits
/// (Redis: WATCH/MULTI/EXEC, Kevy: Lua, in-process: a single
/// DashMap entry lock).
pub fn evaluate_bucket(
    state: Bucket,
    now_unix_secs: u64,
    config: &TokenBucketConfig,
) -> (Bucket, bool) {
    // saturating subtraction — if the clock went backwards (or the
    // bucket is brand new from a "now" lower than its last_refill),
    // treat elapsed as 0 rather than panic on overflow.
    let elapsed_secs = now_unix_secs.saturating_sub(state.last_refill_unix_secs) as f64;

    let refilled =
        (state.tokens + elapsed_secs * config.refill_rate).min(f64::from(config.capacity));

    let (new_tokens, allowed) = if refilled >= 1.0 {
        (refilled - 1.0, true)
    } else {
        (refilled, false)
    };

    (
        Bucket {
            tokens: new_tokens,
            last_refill_unix_secs: now_unix_secs,
        },
        allowed,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(capacity: u32, refill_rate: f64) -> TokenBucketConfig {
        TokenBucketConfig {
            capacity,
            refill_rate,
        }
    }

    fn full(capacity: u32, now: u64) -> Bucket {
        Bucket {
            tokens: f64::from(capacity),
            last_refill_unix_secs: now,
        }
    }

    #[test]
    fn allow_within_capacity() {
        let mut bucket = full(3, 100);
        let config = cfg(3, 0.0);

        for _ in 0..3 {
            let (next, allowed) = evaluate_bucket(bucket, 100, &config);
            assert!(allowed);
            bucket = next;
        }
    }

    #[test]
    fn reject_over_capacity() {
        let mut bucket = full(2, 100);
        let config = cfg(2, 0.0);

        let (b, allowed) = evaluate_bucket(bucket, 100, &config);
        assert!(allowed);
        bucket = b;

        let (b, allowed) = evaluate_bucket(bucket, 100, &config);
        assert!(allowed);
        bucket = b;

        let (_, allowed) = evaluate_bucket(bucket, 100, &config);
        assert!(!allowed);
    }

    #[test]
    fn refill_over_time() {
        let mut bucket = full(1, 100);
        let config = cfg(1, 1.0);

        let (b, allowed) = evaluate_bucket(bucket, 100, &config);
        assert!(allowed);
        bucket = b;

        let (b, allowed) = evaluate_bucket(bucket, 100, &config);
        assert!(!allowed);
        bucket = b;

        // 1 second later: should have refilled 1 token
        let (b, allowed) = evaluate_bucket(bucket, 101, &config);
        assert!(allowed);
        bucket = b;

        let (_, allowed) = evaluate_bucket(bucket, 101, &config);
        assert!(!allowed);
    }

    #[test]
    fn refill_capped_at_capacity() {
        let mut bucket = full(3, 100);
        let config = cfg(3, 10.0);

        let (b, _) = evaluate_bucket(bucket, 100, &config);
        bucket = b;

        // wait 100 seconds — refill would be 1000 tokens, but capped at 3
        // should allow exactly 3 (capacity), not more
        for _ in 0..3 {
            let (b, allowed) = evaluate_bucket(bucket, 200, &config);
            assert!(allowed);
            bucket = b;
        }
        let (_, allowed) = evaluate_bucket(bucket, 200, &config);
        assert!(!allowed);
    }

    #[test]
    fn zero_refill_never_recovers() {
        let mut bucket = full(2, 100);
        let config = cfg(2, 0.0);

        let (b, _) = evaluate_bucket(bucket, 100, &config);
        bucket = b;
        let (b, _) = evaluate_bucket(bucket, 100, &config);
        bucket = b;

        // far in the future
        let (_, allowed) = evaluate_bucket(bucket, 100 + 3600, &config);
        assert!(!allowed);
    }

    #[test]
    fn fresh_bucket_starts_full() {
        let mut bucket = full(5, 100);
        let config = cfg(5, 0.0);

        let mut allowed_count = 0;
        for _ in 0..10 {
            let (b, allowed) = evaluate_bucket(bucket, 100, &config);
            if allowed {
                allowed_count += 1;
            }
            bucket = b;
        }
        assert_eq!(allowed_count, 5);
    }

    #[test]
    fn last_refill_updated_on_every_call() {
        let bucket = full(1, 100);
        let config = cfg(1, 1.0);

        let (next, _) = evaluate_bucket(bucket, 150, &config);
        assert_eq!(next.last_refill_unix_secs, 150);
    }

    #[test]
    fn backward_clock_does_not_panic() {
        let bucket = Bucket {
            tokens: 0.5,
            last_refill_unix_secs: 100,
        };
        let config = cfg(2, 1.0);

        // "now" went backwards — saturating_sub gives 0 elapsed, no refill
        let (next, allowed) = evaluate_bucket(bucket, 50, &config);
        assert!(!allowed);
        // tokens unchanged (0.5)
        assert!((next.tokens - 0.5).abs() < 1e-9);
        assert_eq!(next.last_refill_unix_secs, 50);
    }

    #[test]
    fn fractional_refill_accumulates() {
        let mut bucket = full(2, 100);
        let config = cfg(2, 0.5); // 1 token per 2 seconds

        let (b, _) = evaluate_bucket(bucket, 100, &config);
        bucket = b;
        let (b, _) = evaluate_bucket(bucket, 100, &config);
        bucket = b;
        // drained
        let (b, allowed) = evaluate_bucket(bucket, 100, &config);
        assert!(!allowed);
        bucket = b;

        // 1 sec later: 0.5 tokens, not enough
        let (b, allowed) = evaluate_bucket(bucket, 101, &config);
        assert!(!allowed);
        bucket = b;

        // 2 more sec (3 total): 0.5 + 1.0 = 1.5, allow once, leave 0.5
        let (b, allowed) = evaluate_bucket(bucket, 103, &config);
        assert!(allowed);
        bucket = b;

        let (_, allowed) = evaluate_bucket(bucket, 103, &config);
        assert!(!allowed);
    }

    #[test]
    fn very_small_time_increment_no_refill_to_one() {
        let bucket = Bucket {
            tokens: 0.0,
            last_refill_unix_secs: 100,
        };
        let config = cfg(1, 1.0);

        // same second — no refill, still empty
        let (next, allowed) = evaluate_bucket(bucket, 100, &config);
        assert!(!allowed);
        assert!(next.tokens < 1e-9);
    }

    #[test]
    fn large_capacity_drains_then_rejects() {
        let mut bucket = full(1_000, 100);
        let config = cfg(1_000, 0.0);

        for _ in 0..1_000 {
            let (b, allowed) = evaluate_bucket(bucket, 100, &config);
            assert!(allowed);
            bucket = b;
        }
        let (_, allowed) = evaluate_bucket(bucket, 100, &config);
        assert!(!allowed);
    }
}
