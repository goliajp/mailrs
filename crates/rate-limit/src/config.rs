//! Token-bucket configuration.
//!
//! [`TokenBucketConfig`] describes the shape of every bucket the
//! [`crate::RateLimitStore`] implementation owns: how big the bucket is
//! and how fast it refills. Backends apply the same config to every key —
//! per-key tiers are the consumer's responsibility (compose multiple
//! stores or wrap a single store and look up the right config before
//! `check`).
//!
//! ```
//! use mailrs_rate_limit::TokenBucketConfig;
//!
//! let strict = TokenBucketConfig {
//!     capacity: 5,
//!     refill_rate: 1.0, // one token per second
//! };
//! assert_eq!(strict.capacity, 5);
//!
//! // Default is 10 tokens, 1/sec refill — the same shape that fronts
//! // mailrs's SMTP connect path.
//! let default = TokenBucketConfig::default();
//! assert_eq!(default.capacity, 10);
//! assert!((default.refill_rate - 1.0).abs() < f64::EPSILON);
//! ```

/// Token-bucket parameters.
///
/// `capacity` is the burst size — the maximum number of tokens the bucket
/// can hold at any moment. `refill_rate` is how many tokens are added per
/// second when the bucket is below capacity.
///
/// At steady state, a key that consistently exceeds `refill_rate`
/// requests/sec will eventually be throttled; spikes shorter than
/// `capacity / refill_rate` seconds pass through unchanged.
#[derive(Debug, Clone)]
pub struct TokenBucketConfig {
    /// Maximum tokens the bucket can hold (the burst size).
    ///
    /// A fresh bucket starts at this value; once drained, the bucket
    /// refills at `refill_rate` per second, capped at `capacity`.
    pub capacity: u32,

    /// Tokens added per second (fractional rates are fine).
    ///
    /// `1.0` means "one request per second on average"; `0.5` means
    /// "one request every two seconds". `0.0` disables refill — once
    /// the bucket is drained it never recovers (useful for first-N-only
    /// allowance).
    pub refill_rate: f64,
}

impl Default for TokenBucketConfig {
    /// 10-burst, 1 token / sec refill. Matches the historical mailrs
    /// inbound default.
    fn default() -> Self {
        Self {
            capacity: 10,
            refill_rate: 1.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let config = TokenBucketConfig::default();
        assert_eq!(config.capacity, 10);
        assert!((config.refill_rate - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn config_clone() {
        let config = TokenBucketConfig {
            capacity: 42,
            refill_rate: 3.5,
        };
        let cloned = config.clone();
        assert_eq!(cloned.capacity, 42);
        assert!((cloned.refill_rate - 3.5).abs() < f64::EPSILON);
    }

    #[test]
    fn zero_refill_is_valid() {
        let config = TokenBucketConfig {
            capacity: 1,
            refill_rate: 0.0,
        };
        assert_eq!(config.capacity, 1);
        assert_eq!(config.refill_rate, 0.0);
    }
}
