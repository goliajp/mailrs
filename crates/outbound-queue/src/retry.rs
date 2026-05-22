//! Exponential-backoff retry helpers for SMTP outbound delivery.
//!
//! Backed by [`mailrs_backoff::Backoff::smtp_outbound`]. Replaces the
//! previous hardcoded 8-slot schedule with a clean parametric curve;
//! gains Full jitter to avoid thundering-herd on shared MX downtime.

use mailrs_backoff::Backoff;

/// Retry delay in seconds for attempt `n` (0-indexed), **deterministic
/// schedule** (no jitter). For jittered scheduling use [`retry_delay_secs_jittered`].
///
/// Backed by `Backoff::smtp_outbound`: initial 60s, 2.5× growth, capped
/// at 8 hours. Compared to the pre-1.1 hardcoded schedule
/// `[60, 300, 900, 1800, 3600, 7200, 14400, 28800]`, this curve grows
/// slightly faster in the early attempts but converges at the same
/// 8-hour cap.
pub fn retry_delay_secs(attempt: u32) -> u64 {
    Backoff::smtp_outbound().base_delay(attempt).as_secs()
}

/// Retry delay in seconds with Full jitter applied. Caller supplies a
/// random seed (e.g. `Instant::now().elapsed().as_nanos() as u64` or
/// `rand::random::<u64>()`). The same `(attempt, seed)` pair always
/// yields the same delay, so tests can reproduce.
///
/// Use this in production schedulers to spread retry traffic and
/// avoid synchronized retry bursts across queue rows that all failed
/// at the same time (MX outage, DNS hiccup).
pub fn retry_delay_secs_jittered(attempt: u32, seed: u64) -> u64 {
    Backoff::smtp_outbound().delay(attempt, seed).as_secs()
}

/// Should this attempt give up and bounce the message? Returns `true`
/// when `attempt >= max_attempts`. Equivalent to
/// `mailrs_backoff::Backoff::should_give_up`; kept for back-compat.
pub fn should_bounce(attempt: u32, max_attempts: u32) -> bool {
    Backoff::should_give_up(attempt, max_attempts)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The new schedule under `Backoff::smtp_outbound` (initial 60s,
    /// 2.5× growth, capped at 8h = 28800s).
    ///
    /// Computed by hand: 60 × 2.5^n, capped at 28800.
    ///   n=0: 60
    ///   n=1: 150
    ///   n=2: 375
    ///   n=3: 937 (937.5 floored)
    ///   n=4: 2343 (2343.75)
    ///   n=5: 5859 (5859.375)
    ///   n=6: 14648 (14648.4375)
    ///   n=7: 28800 (would be 36621, capped)
    #[test]
    fn delay_schedule_new_curve() {
        assert_eq!(retry_delay_secs(0), 60);
        assert_eq!(retry_delay_secs(1), 150);
        assert_eq!(retry_delay_secs(2), 375);
        // Floating point: 60 * 2.5^3 = 937.5 → 937 as u64
        let d3 = retry_delay_secs(3);
        assert!((937..=938).contains(&d3), "expected ~937, got {d3}");
        let d4 = retry_delay_secs(4);
        assert!((2343..=2344).contains(&d4), "expected ~2343, got {d4}");
    }

    #[test]
    fn cap_at_8h() {
        // Anything past attempt ~6 hits the 8h cap.
        assert_eq!(retry_delay_secs(7), 28800);
        assert_eq!(retry_delay_secs(8), 28800);
        assert_eq!(retry_delay_secs(100), 28800);
    }

    #[test]
    fn bounce_at_max() {
        assert!(should_bounce(5, 5));
        assert!(should_bounce(10, 5));
    }

    #[test]
    fn no_bounce_below_max() {
        assert!(!should_bounce(0, 5));
        assert!(!should_bounce(4, 5));
    }

    #[test]
    fn delay_is_monotonically_increasing() {
        let mut prev = 0u64;
        for i in 0..8u32 {
            let d = retry_delay_secs(i);
            assert!(d > prev, "delay at slot {i} ({d}) is not greater than previous ({prev})");
            prev = d;
        }
    }

    #[test]
    fn should_bounce_boundary_exact() {
        assert!(should_bounce(1, 1));
        assert!(should_bounce(0, 0));
    }

    #[test]
    fn should_bounce_large_values() {
        assert!(!should_bounce(u32::MAX - 1, u32::MAX));
        assert!(should_bounce(u32::MAX, u32::MAX));
    }

    #[test]
    fn delay_first_attempt_is_one_minute() {
        assert_eq!(retry_delay_secs(0), 60);
    }

    #[test]
    fn delay_cap_at_various_overflow_attempts() {
        for attempt in [8, 9, 10, 50, 100, 255, 1000, u32::MAX] {
            assert_eq!(
                retry_delay_secs(attempt),
                28800,
                "attempt {attempt} should be capped at 8h"
            );
        }
    }

    #[test]
    fn delay_each_step_grows() {
        for i in 1..7u32 {
            let prev = retry_delay_secs(i - 1);
            let curr = retry_delay_secs(i);
            assert!(curr >= prev * 2, "step {i}: {curr} should be at least 2× {prev}");
        }
    }

    #[test]
    fn delay_minimum_is_60_seconds() {
        assert_eq!(retry_delay_secs(0), 60);
        // No call to retry_delay_secs should ever return less than 60.
        for n in 0..8u32 {
            assert!(retry_delay_secs(n) >= 60, "attempt {n} returned < 60");
        }
    }

    // ===== jitter variant tests =====

    #[test]
    fn jittered_delay_within_full_range() {
        let base = retry_delay_secs(3);
        for seed in 0..50u64 {
            let d = retry_delay_secs_jittered(3, seed);
            // Full jitter: 0 <= d < base
            assert!(d < base, "seed {seed}: jittered {d} >= base {base}");
        }
    }

    #[test]
    fn jittered_deterministic_with_same_seed() {
        assert_eq!(
            retry_delay_secs_jittered(3, 42),
            retry_delay_secs_jittered(3, 42),
        );
    }

    #[test]
    fn jittered_attempt_zero_bounded() {
        // base for attempt=0 is 60s. Full jitter: result in [0, 60).
        for seed in 0..20u64 {
            let d = retry_delay_secs_jittered(0, seed);
            assert!(d < 60, "seed {seed}: {d} >= 60");
        }
    }

    #[test]
    fn should_bounce_one_attempt_max() {
        assert!(!should_bounce(0, 1));
        assert!(should_bounce(1, 1));
        assert!(should_bounce(2, 1));
    }

    #[test]
    fn should_bounce_high_max_attempts() {
        let max = 100;
        assert!(!should_bounce(99, max));
        assert!(should_bounce(100, max));
        assert!(should_bounce(101, max));
    }

    #[test]
    fn delay_returns_u64() {
        let d: u64 = retry_delay_secs(0);
        assert_eq!(d, 60u64);
    }
}
