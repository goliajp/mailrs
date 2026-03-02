use std::net::IpAddr;
use std::time::Instant;

use dashmap::DashMap;

#[derive(Debug, Clone)]
pub struct TokenBucketConfig {
    /// max tokens in bucket
    pub capacity: u32,
    /// tokens added per second
    pub refill_rate: f64,
}

impl Default for TokenBucketConfig {
    fn default() -> Self {
        Self {
            capacity: 10,
            refill_rate: 1.0,
        }
    }
}

#[derive(Debug)]
struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

pub struct RateLimiter {
    config: TokenBucketConfig,
    buckets: DashMap<IpAddr, Bucket>,
}

impl RateLimiter {
    pub fn new(config: TokenBucketConfig) -> Self {
        Self {
            config,
            buckets: DashMap::new(),
        }
    }

    /// try to consume a token, returns true if allowed
    pub fn check(&self, ip: IpAddr) -> bool {
        self.check_at(ip, Instant::now())
    }

    /// testable version with explicit timestamp
    fn check_at(&self, ip: IpAddr, now: Instant) -> bool {
        let mut entry = self.buckets.entry(ip).or_insert_with(|| Bucket {
            tokens: self.config.capacity as f64,
            last_refill: now,
        });

        let bucket = entry.value_mut();
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * self.config.refill_rate)
            .min(self.config.capacity as f64);
        bucket.last_refill = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// remove entries that haven't been seen since `before`
    pub fn cleanup_stale(&self, before: Instant) {
        self.buckets
            .retain(|_, bucket| bucket.last_refill >= before);
    }

    pub fn len(&self) -> usize {
        self.buckets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buckets.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn localhost() -> IpAddr {
        IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
    }

    #[test]
    fn allow_within_capacity() {
        let limiter = RateLimiter::new(TokenBucketConfig {
            capacity: 3,
            refill_rate: 0.0,
        });
        assert!(limiter.check(localhost()));
        assert!(limiter.check(localhost()));
        assert!(limiter.check(localhost()));
    }

    #[test]
    fn reject_over_capacity() {
        let limiter = RateLimiter::new(TokenBucketConfig {
            capacity: 2,
            refill_rate: 0.0,
        });
        assert!(limiter.check(localhost()));
        assert!(limiter.check(localhost()));
        assert!(!limiter.check(localhost()));
    }

    #[test]
    fn refill_over_time() {
        let limiter = RateLimiter::new(TokenBucketConfig {
            capacity: 1,
            refill_rate: 1.0,
        });
        let t0 = Instant::now();

        // consume the only token
        assert!(limiter.check_at(localhost(), t0));
        assert!(!limiter.check_at(localhost(), t0));

        // after 1 second, should have 1 token again
        let t1 = t0 + Duration::from_secs(1);
        assert!(limiter.check_at(localhost(), t1));
        assert!(!limiter.check_at(localhost(), t1));
    }

    #[test]
    fn cleanup_stale() {
        let limiter = RateLimiter::new(TokenBucketConfig::default());
        let t0 = Instant::now();
        limiter.check_at(localhost(), t0);
        assert_eq!(limiter.len(), 1);

        // cleanup entries older than t0 + 1s
        limiter.cleanup_stale(t0 + Duration::from_secs(1));
        assert_eq!(limiter.len(), 0);
    }
}
