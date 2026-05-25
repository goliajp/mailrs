#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use std::time::Duration;

/// Jitter policy applied to each delay sample.
///
/// Following AWS Architecture Blog's "Exponential Backoff and Jitter"
/// taxonomy:
///
/// - **None** — deterministic exponential; bad at scale because all
///   clients retry at the same instant ("thundering herd")
/// - **Equal** — half the delay is fixed, half is uniformly random;
///   bounded but smoothed
/// - **Full** — delay is uniformly random in `[0, base]`; AWS's
///   recommended default. Maximum spread.
///
/// See <https://aws.amazon.com/blogs/architecture/exponential-backoff-and-jitter/>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Jitter {
    /// No jitter — deterministic delay schedule.
    None,
    /// Half fixed, half random: `delay = base/2 + uniform(0, base/2)`.
    Equal,
    /// Fully random: `delay = uniform(0, base)`.
    Full,
}

/// Exponential-backoff configuration.
///
/// `delay(attempt)` returns `min(initial × multiplier^attempt, max)`,
/// then applies the configured [`Jitter`] policy.
///
/// All fields are `pub` so you can construct + tune directly, or use
/// one of the preset constructors ([`Backoff::smtp_outbound`],
/// [`Backoff::auth_lockout`], etc.).
#[derive(Debug, Clone, Copy)]
pub struct Backoff {
    /// Delay for `attempt = 0`.
    pub initial: Duration,
    /// Geometric growth factor between attempts. Common values: 2.0
    /// (doubling) or 1.5 (gentler ramp).
    pub multiplier: f64,
    /// Hard ceiling on delay — once `initial × multiplier^attempt`
    /// would exceed `max`, return `max`.
    pub max: Duration,
    /// Jitter policy applied to the computed delay.
    pub jitter: Jitter,
}

impl Default for Backoff {
    /// Default: 1 second initial, 2× growth, 1 hour cap, Full jitter.
    /// Reasonable for generic HTTP/RPC retry loops; pick a preset for
    /// other use cases.
    fn default() -> Self {
        Self {
            initial: Duration::from_secs(1),
            multiplier: 2.0,
            max: Duration::from_secs(3600),
            jitter: Jitter::Full,
        }
    }
}

impl Backoff {
    /// Construct with explicit values. Use the preset constructors
    /// below for common shapes.
    pub fn new(initial: Duration, multiplier: f64, max: Duration, jitter: Jitter) -> Self {
        Self {
            initial,
            multiplier,
            max,
            jitter,
        }
    }

    /// Preset for SMTP outbound delivery retries: 60s initial, 2.5×
    /// growth, 8-hour cap, Full jitter. Mirrors the legacy
    /// `mailrs-outbound-queue` schedule shape.
    pub fn smtp_outbound() -> Self {
        Self {
            initial: Duration::from_secs(60),
            multiplier: 2.5,
            max: Duration::from_secs(8 * 3600),
            jitter: Jitter::Full,
        }
    }

    /// Preset for failed-auth lockout: 30min initial, 2× growth,
    /// 24-hour cap, None jitter (lockouts are deterministic by design —
    /// you want every offender to see exactly the same penalty).
    pub fn auth_lockout() -> Self {
        Self {
            initial: Duration::from_secs(30 * 60),
            multiplier: 2.0,
            max: Duration::from_secs(24 * 3600),
            jitter: Jitter::None,
        }
    }

    /// Preset for webhook delivery retries: 60s initial, 2× growth,
    /// 6-hour cap, Equal jitter. Smoother than Full jitter so subscriber
    /// endpoints see a less spiky retry pattern.
    pub fn webhook() -> Self {
        Self {
            initial: Duration::from_secs(60),
            multiplier: 2.0,
            max: Duration::from_secs(6 * 3600),
            jitter: Jitter::Equal,
        }
    }

    /// Base delay for `attempt` BEFORE jitter is applied. Useful when
    /// you want to log "scheduled delay" without the random part.
    pub fn base_delay(&self, attempt: u32) -> Duration {
        // Early bail for very-high attempts. multiplier^attempt for
        // attempt > 64 with multiplier >= 1.0 is astronomically larger
        // than any realistic max. Skip the float math + the i32 cast
        // (which would wrap for attempt > i32::MAX).
        if attempt > 64 && self.multiplier >= 1.0 {
            return self.max;
        }
        // For attempt <= 64 the cast to i32 is always safe.
        let initial_ns = self.initial.as_nanos() as f64;
        let max_ns = self.max.as_nanos() as f64;
        let factor = self.multiplier.powi(attempt as i32);
        let raw = initial_ns * factor;
        let clamped = raw.min(max_ns).max(0.0);
        Duration::from_nanos(clamped as u64)
    }

    /// Compute the delay for `attempt`, applying the configured jitter.
    /// `seed` is a caller-supplied source of randomness — usually
    /// derived from a fresh `rand::random::<u64>()` or
    /// `std::time::Instant::now().elapsed().as_nanos()`. The crate
    /// itself has no RNG dependency.
    ///
    /// For [`Jitter::None`] the seed is ignored. For Equal/Full it
    /// drives a deterministic mapping (same seed → same delay), so
    /// tests can be reproducible.
    pub fn delay(&self, attempt: u32, seed: u64) -> Duration {
        let base = self.base_delay(attempt);
        match self.jitter {
            Jitter::None => base,
            Jitter::Equal => {
                // half fixed + half random uniform [0, base/2]
                let half_ns = base.as_nanos() as u64 / 2;
                let random_part = scale_random(seed, half_ns);
                Duration::from_nanos(half_ns + random_part)
            }
            Jitter::Full => {
                let base_ns = base.as_nanos() as u64;
                let random = scale_random(seed, base_ns);
                Duration::from_nanos(random)
            }
        }
    }

    /// Convenience: should this attempt give up?
    /// Returns `true` if `attempt >= max_attempts`.
    pub fn should_give_up(attempt: u32, max_attempts: u32) -> bool {
        attempt >= max_attempts
    }
}

/// Map a u64 seed to a uniform sample in `[0, ceiling)`.
/// `ceiling == 0` returns 0 (avoids division by zero).
#[inline]
fn scale_random(seed: u64, ceiling: u64) -> u64 {
    if ceiling == 0 {
        return 0;
    }
    // SplitMix64 step — fast, reasonable distribution. Not crypto.
    let mut x = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    x % ceiling
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_construction() {
        let b = Backoff::default();
        assert_eq!(b.initial, Duration::from_secs(1));
        assert_eq!(b.multiplier, 2.0);
        assert_eq!(b.max, Duration::from_secs(3600));
        assert_eq!(b.jitter, Jitter::Full);
    }

    #[test]
    fn base_delay_grows_exponentially() {
        let b = Backoff {
            initial: Duration::from_secs(1),
            multiplier: 2.0,
            max: Duration::from_secs(3600),
            jitter: Jitter::None,
        };
        assert_eq!(b.base_delay(0), Duration::from_secs(1));
        assert_eq!(b.base_delay(1), Duration::from_secs(2));
        assert_eq!(b.base_delay(2), Duration::from_secs(4));
        assert_eq!(b.base_delay(3), Duration::from_secs(8));
        assert_eq!(b.base_delay(10), Duration::from_secs(1024));
    }

    #[test]
    fn base_delay_caps_at_max() {
        let b = Backoff {
            initial: Duration::from_secs(60),
            multiplier: 2.0,
            max: Duration::from_secs(3600),
            jitter: Jitter::None,
        };
        // 60 × 2^7 = 7680 > 3600 → cap at 3600
        assert_eq!(b.base_delay(7), Duration::from_secs(3600));
        assert_eq!(b.base_delay(100), Duration::from_secs(3600));
        assert_eq!(b.base_delay(u32::MAX), Duration::from_secs(3600));
    }

    #[test]
    fn jitter_none_is_deterministic() {
        let b = Backoff {
            initial: Duration::from_secs(60),
            multiplier: 2.0,
            max: Duration::from_secs(3600),
            jitter: Jitter::None,
        };
        // Two calls with different seeds → same result.
        assert_eq!(b.delay(3, 0), b.delay(3, 999_999));
        assert_eq!(b.delay(3, 0), b.base_delay(3));
    }

    #[test]
    fn jitter_equal_returns_at_least_half_base() {
        let b = Backoff {
            initial: Duration::from_secs(100),
            multiplier: 2.0,
            max: Duration::from_secs(10_000),
            jitter: Jitter::Equal,
        };
        let base = b.base_delay(2);
        for seed in 0..100u64 {
            let d = b.delay(2, seed);
            assert!(
                d >= base / 2,
                "seed {seed}: d={d:?} >= base/2 {:?}",
                base / 2
            );
            assert!(d <= base, "seed {seed}: d={d:?} <= base {base:?}");
        }
    }

    #[test]
    fn jitter_full_returns_in_zero_to_base() {
        let b = Backoff {
            initial: Duration::from_secs(100),
            multiplier: 2.0,
            max: Duration::from_secs(10_000),
            jitter: Jitter::Full,
        };
        let base = b.base_delay(2);
        for seed in 0..100u64 {
            let d = b.delay(2, seed);
            assert!(d < base, "seed {seed}: d={d:?} < base {base:?}");
        }
    }

    #[test]
    fn jitter_deterministic_with_same_seed() {
        let b = Backoff::smtp_outbound();
        // Same seed should always produce the same delay.
        assert_eq!(b.delay(3, 42), b.delay(3, 42));
        assert_eq!(b.delay(0, 12345), b.delay(0, 12345));
    }

    #[test]
    fn smtp_outbound_preset() {
        let b = Backoff::smtp_outbound();
        assert_eq!(b.initial, Duration::from_secs(60));
        assert_eq!(b.multiplier, 2.5);
        assert_eq!(b.max, Duration::from_secs(8 * 3600));
        assert_eq!(b.jitter, Jitter::Full);
    }

    #[test]
    fn auth_lockout_preset() {
        let b = Backoff::auth_lockout();
        assert_eq!(b.initial, Duration::from_secs(30 * 60));
        assert_eq!(b.multiplier, 2.0);
        assert_eq!(b.max, Duration::from_secs(24 * 3600));
        assert_eq!(b.jitter, Jitter::None);
    }

    #[test]
    fn webhook_preset() {
        let b = Backoff::webhook();
        assert_eq!(b.initial, Duration::from_secs(60));
        assert_eq!(b.multiplier, 2.0);
        assert_eq!(b.max, Duration::from_secs(6 * 3600));
        assert_eq!(b.jitter, Jitter::Equal);
    }

    #[test]
    fn should_give_up_at_max() {
        assert!(Backoff::should_give_up(5, 5));
        assert!(Backoff::should_give_up(10, 5));
        assert!(!Backoff::should_give_up(0, 5));
        assert!(!Backoff::should_give_up(4, 5));
    }

    #[test]
    fn should_give_up_zero_max_immediate() {
        assert!(Backoff::should_give_up(0, 0));
    }

    #[test]
    fn should_give_up_max_at_u32_boundary() {
        assert!(!Backoff::should_give_up(u32::MAX - 1, u32::MAX));
        assert!(Backoff::should_give_up(u32::MAX, u32::MAX));
    }

    #[test]
    fn base_delay_attempt_zero_is_initial() {
        let b = Backoff::smtp_outbound();
        assert_eq!(b.base_delay(0), b.initial);
    }

    #[test]
    fn multiplier_below_one_decays() {
        // multiplier=0.5 → delays decrease (unusual but should not panic)
        let b = Backoff {
            initial: Duration::from_secs(100),
            multiplier: 0.5,
            max: Duration::from_secs(3600),
            jitter: Jitter::None,
        };
        assert_eq!(b.base_delay(0), Duration::from_secs(100));
        assert_eq!(b.base_delay(1), Duration::from_secs(50));
        assert_eq!(b.base_delay(2), Duration::from_secs(25));
    }

    #[test]
    fn jitter_full_with_zero_base_returns_zero() {
        // attempt that yields zero base (initial=0)
        let b = Backoff {
            initial: Duration::ZERO,
            multiplier: 2.0,
            max: Duration::from_secs(3600),
            jitter: Jitter::Full,
        };
        assert_eq!(b.delay(5, 42), Duration::ZERO);
    }

    #[test]
    fn scale_random_zero_ceiling_returns_zero() {
        assert_eq!(scale_random(123, 0), 0);
        assert_eq!(scale_random(u64::MAX, 0), 0);
    }

    #[test]
    fn scale_random_distribution_spread() {
        // SplitMix64 should produce well-distributed results.
        // Sample 1000 seeds, check we hit at least 90% of bucket coverage.
        let buckets = 10;
        let mut hits = [false; 10];
        for seed in 0..1000u64 {
            let v = scale_random(seed, buckets);
            hits[v as usize] = true;
        }
        let coverage = hits.iter().filter(|h| **h).count();
        assert!(coverage >= 9, "only hit {coverage}/{buckets} buckets");
    }

    #[test]
    fn delay_caps_under_jitter() {
        // Even with jitter, delay should never exceed max.
        let b = Backoff::smtp_outbound();
        for attempt in 0..30u32 {
            for seed in [0u64, 1, 42, 100, 12345, u64::MAX] {
                let d = b.delay(attempt, seed);
                assert!(
                    d <= b.max,
                    "attempt={attempt} seed={seed} d={d:?} > max={:?}",
                    b.max
                );
            }
        }
    }

    #[test]
    fn very_high_attempt_doesnt_overflow() {
        let b = Backoff::smtp_outbound();
        // attempt=100 → multiplier^100 is astronomical, but cap saves us
        let d = b.delay(100, 42);
        assert!(d <= b.max);
    }

    #[test]
    fn clone_and_copy_work() {
        let a = Backoff::webhook();
        let b = a;
        assert_eq!(a.initial, b.initial);
        assert_eq!(a.multiplier, b.multiplier);
        let c = a;
        assert_eq!(c.max, a.max);
    }
}
