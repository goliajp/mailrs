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
        bucket.tokens =
            (bucket.tokens + elapsed * self.config.refill_rate).min(self.config.capacity as f64);
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
    use std::net::{Ipv4Addr, Ipv6Addr};
    use std::sync::Arc;
    use std::time::Duration;

    fn localhost() -> IpAddr {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    }

    fn ip(last_octet: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(10, 0, 0, last_octet))
    }

    // --- existing tests ---

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

    // --- TokenBucketConfig tests ---

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

    // --- capacity boundary tests ---

    #[test]
    fn capacity_one_allows_single_request() {
        let limiter = RateLimiter::new(TokenBucketConfig {
            capacity: 1,
            refill_rate: 0.0,
        });
        assert!(limiter.check(localhost()));
        assert!(!limiter.check(localhost()));
    }

    #[test]
    fn exhaust_exact_capacity_then_reject() {
        let limiter = RateLimiter::new(TokenBucketConfig {
            capacity: 5,
            refill_rate: 0.0,
        });
        for _ in 0..5 {
            assert!(limiter.check(localhost()));
        }
        // 6th must be rejected
        assert!(!limiter.check(localhost()));
    }

    #[test]
    fn multiple_rejections_after_exhaustion() {
        let limiter = RateLimiter::new(TokenBucketConfig {
            capacity: 1,
            refill_rate: 0.0,
        });
        assert!(limiter.check(localhost()));
        // repeated rejections should not panic or change state
        for _ in 0..10 {
            assert!(!limiter.check(localhost()));
        }
    }

    // --- refill behavior ---

    #[test]
    fn partial_refill_not_enough_for_token() {
        let limiter = RateLimiter::new(TokenBucketConfig {
            capacity: 1,
            refill_rate: 1.0,
        });
        let t0 = Instant::now();
        assert!(limiter.check_at(localhost(), t0));
        assert!(!limiter.check_at(localhost(), t0));

        // 0.5 seconds: 0.5 tokens refilled, still < 1.0
        let t_half = t0 + Duration::from_millis(500);
        assert!(!limiter.check_at(localhost(), t_half));
    }

    #[test]
    fn refill_capped_at_capacity() {
        let limiter = RateLimiter::new(TokenBucketConfig {
            capacity: 3,
            refill_rate: 10.0,
        });
        let t0 = Instant::now();
        // consume 1 token
        assert!(limiter.check_at(localhost(), t0));

        // wait 100 seconds — refill would be 1000 tokens, but capped at 3
        let t_later = t0 + Duration::from_secs(100);
        // should allow exactly 3 (capacity), not more
        assert!(limiter.check_at(localhost(), t_later));
        assert!(limiter.check_at(localhost(), t_later));
        assert!(limiter.check_at(localhost(), t_later));
        assert!(!limiter.check_at(localhost(), t_later));
    }

    #[test]
    fn high_refill_rate_restores_quickly() {
        let limiter = RateLimiter::new(TokenBucketConfig {
            capacity: 10,
            refill_rate: 100.0,
        });
        let t0 = Instant::now();

        // exhaust all tokens
        for _ in 0..10 {
            assert!(limiter.check_at(localhost(), t0));
        }
        assert!(!limiter.check_at(localhost(), t0));

        // after 100ms at 100 tokens/sec = 10 tokens refilled
        let t1 = t0 + Duration::from_millis(100);
        for _ in 0..10 {
            assert!(limiter.check_at(localhost(), t1));
        }
        assert!(!limiter.check_at(localhost(), t1));
    }

    #[test]
    fn zero_refill_rate_never_recovers() {
        let limiter = RateLimiter::new(TokenBucketConfig {
            capacity: 2,
            refill_rate: 0.0,
        });
        let t0 = Instant::now();
        assert!(limiter.check_at(localhost(), t0));
        assert!(limiter.check_at(localhost(), t0));
        assert!(!limiter.check_at(localhost(), t0));

        // even after a long time, no recovery
        let t_far = t0 + Duration::from_secs(3600);
        assert!(!limiter.check_at(localhost(), t_far));
    }

    #[test]
    fn fractional_refill_accumulates() {
        let limiter = RateLimiter::new(TokenBucketConfig {
            capacity: 2,
            refill_rate: 0.5, // 1 token every 2 seconds
        });
        let t0 = Instant::now();
        // drain both
        assert!(limiter.check_at(localhost(), t0));
        assert!(limiter.check_at(localhost(), t0));
        assert!(!limiter.check_at(localhost(), t0));

        // after 1 second: 0.5 tokens, not enough
        let t1 = t0 + Duration::from_secs(1);
        assert!(!limiter.check_at(localhost(), t1));

        // after 2 more seconds (total 3s from t0): 0.5 + 1.0 = 1.5, but we tried at t1
        // from t1, 2 more seconds = 1.0 token refilled
        let t3 = t1 + Duration::from_secs(2);
        assert!(limiter.check_at(localhost(), t3));
        assert!(!limiter.check_at(localhost(), t3));
    }

    // --- per-IP isolation ---

    #[test]
    fn different_ips_have_separate_buckets() {
        let limiter = RateLimiter::new(TokenBucketConfig {
            capacity: 1,
            refill_rate: 0.0,
        });
        let ip_a = ip(1);
        let ip_b = ip(2);

        assert!(limiter.check_at(ip_a, Instant::now()));
        assert!(!limiter.check_at(ip_a, Instant::now()));
        // ip_b still has its own bucket
        assert!(limiter.check_at(ip_b, Instant::now()));
        assert!(!limiter.check_at(ip_b, Instant::now()));
    }

    #[test]
    fn many_ips_tracked_independently() {
        let limiter = RateLimiter::new(TokenBucketConfig {
            capacity: 1,
            refill_rate: 0.0,
        });
        let t0 = Instant::now();

        for i in 1..=100 {
            let addr = ip(i);
            assert!(limiter.check_at(addr, t0));
        }
        assert_eq!(limiter.len(), 100);

        // each ip is now exhausted
        for i in 1..=100 {
            let addr = ip(i);
            assert!(!limiter.check_at(addr, t0));
        }
    }

    #[test]
    fn ipv6_address_works() {
        let limiter = RateLimiter::new(TokenBucketConfig {
            capacity: 2,
            refill_rate: 0.0,
        });
        let v6 = IpAddr::V6(Ipv6Addr::LOCALHOST);
        assert!(limiter.check(v6));
        assert!(limiter.check(v6));
        assert!(!limiter.check(v6));
    }

    #[test]
    fn ipv4_and_ipv6_are_separate_buckets() {
        let limiter = RateLimiter::new(TokenBucketConfig {
            capacity: 1,
            refill_rate: 0.0,
        });
        let v4 = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let v6 = IpAddr::V6(Ipv6Addr::LOCALHOST);

        assert!(limiter.check(v4));
        assert!(!limiter.check(v4));
        // v6 is a different bucket
        assert!(limiter.check(v6));
        assert!(!limiter.check(v6));
        assert_eq!(limiter.len(), 2);
    }

    // --- len / is_empty ---

    #[test]
    fn new_limiter_is_empty() {
        let limiter = RateLimiter::new(TokenBucketConfig::default());
        assert!(limiter.is_empty());
        assert_eq!(limiter.len(), 0);
    }

    #[test]
    fn len_grows_with_unique_ips() {
        let limiter = RateLimiter::new(TokenBucketConfig::default());
        assert_eq!(limiter.len(), 0);

        limiter.check_at(ip(1), Instant::now());
        assert_eq!(limiter.len(), 1);
        assert!(!limiter.is_empty());

        limiter.check_at(ip(2), Instant::now());
        assert_eq!(limiter.len(), 2);

        // same ip again doesn't increase len
        limiter.check_at(ip(1), Instant::now());
        assert_eq!(limiter.len(), 2);
    }

    // --- cleanup_stale tests ---

    #[test]
    fn cleanup_preserves_fresh_entries() {
        let limiter = RateLimiter::new(TokenBucketConfig::default());
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_secs(5);

        limiter.check_at(ip(1), t0);
        limiter.check_at(ip(2), t1);
        assert_eq!(limiter.len(), 2);

        // cutoff between t0 and t1: only ip(1) is stale
        let cutoff = t0 + Duration::from_secs(3);
        limiter.cleanup_stale(cutoff);
        assert_eq!(limiter.len(), 1);
    }

    #[test]
    fn cleanup_removes_all_when_all_stale() {
        let limiter = RateLimiter::new(TokenBucketConfig::default());
        let t0 = Instant::now();
        limiter.check_at(ip(1), t0);
        limiter.check_at(ip(2), t0);
        limiter.check_at(ip(3), t0);
        assert_eq!(limiter.len(), 3);

        let far_future = t0 + Duration::from_secs(3600);
        limiter.cleanup_stale(far_future);
        assert!(limiter.is_empty());
    }

    #[test]
    fn cleanup_removes_nothing_when_all_fresh() {
        let limiter = RateLimiter::new(TokenBucketConfig::default());
        let t0 = Instant::now();
        limiter.check_at(ip(1), t0);
        limiter.check_at(ip(2), t0);

        // cutoff is before t0
        let before = t0 - Duration::from_secs(1);
        limiter.cleanup_stale(before);
        assert_eq!(limiter.len(), 2);
    }

    #[test]
    fn cleanup_on_empty_limiter_is_noop() {
        let limiter = RateLimiter::new(TokenBucketConfig::default());
        limiter.cleanup_stale(Instant::now());
        assert!(limiter.is_empty());
    }

    #[test]
    fn cleanup_boundary_exact_timestamp() {
        // entry at exactly the cutoff should be retained (>= comparison)
        let limiter = RateLimiter::new(TokenBucketConfig::default());
        let t0 = Instant::now();
        limiter.check_at(ip(1), t0);

        limiter.cleanup_stale(t0);
        assert_eq!(limiter.len(), 1, "entry at exact cutoff should be retained");
    }

    #[test]
    fn check_after_cleanup_creates_fresh_bucket() {
        let limiter = RateLimiter::new(TokenBucketConfig {
            capacity: 2,
            refill_rate: 0.0,
        });
        let t0 = Instant::now();

        // use up tokens
        limiter.check_at(localhost(), t0);
        limiter.check_at(localhost(), t0);
        assert!(!limiter.check_at(localhost(), t0));

        // cleanup removes it
        limiter.cleanup_stale(t0 + Duration::from_secs(1));
        assert!(limiter.is_empty());

        // new check creates a fresh bucket with full capacity
        let t_new = t0 + Duration::from_secs(2);
        assert!(limiter.check_at(localhost(), t_new));
        assert!(limiter.check_at(localhost(), t_new));
        assert!(!limiter.check_at(localhost(), t_new));
    }

    // --- refill timestamp update ---

    #[test]
    fn last_refill_updated_on_every_check() {
        // verifies that checking updates last_refill, which affects cleanup
        let limiter = RateLimiter::new(TokenBucketConfig {
            capacity: 10,
            refill_rate: 1.0,
        });
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_secs(5);
        let t2 = t0 + Duration::from_secs(10);

        limiter.check_at(ip(1), t0);
        // ip(1) refreshes at t1
        limiter.check_at(ip(1), t1);

        // cleanup at t0+3s: ip(1) was last seen at t1 (>= t0+3s), so it stays
        limiter.cleanup_stale(t0 + Duration::from_secs(3));
        assert_eq!(limiter.len(), 1);

        // cleanup at t2: ip(1) was last seen at t1 (< t2), so removed
        limiter.cleanup_stale(t2);
        assert!(limiter.is_empty());
    }

    // --- concurrent access ---

    #[test]
    fn concurrent_checks_from_same_ip() {
        let limiter = Arc::new(RateLimiter::new(TokenBucketConfig {
            capacity: 100,
            refill_rate: 0.0,
        }));

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let limiter = Arc::clone(&limiter);
                std::thread::spawn(move || {
                    let mut allowed = 0u32;
                    for _ in 0..20 {
                        if limiter.check(localhost()) {
                            allowed += 1;
                        }
                    }
                    allowed
                })
            })
            .collect();

        let total_allowed: u32 = handles.into_iter().map(|h| h.join().unwrap()).sum();
        // exactly 100 tokens should be consumed across all threads
        assert_eq!(
            total_allowed, 100,
            "total allowed should equal capacity"
        );
    }

    #[test]
    fn concurrent_checks_from_different_ips() {
        let limiter = Arc::new(RateLimiter::new(TokenBucketConfig {
            capacity: 5,
            refill_rate: 0.0,
        }));

        let handles: Vec<_> = (1..=20u8)
            .map(|i| {
                let limiter = Arc::clone(&limiter);
                std::thread::spawn(move || {
                    let addr = ip(i);
                    let mut allowed = 0u32;
                    for _ in 0..10 {
                        if limiter.check(addr) {
                            allowed += 1;
                        }
                    }
                    // each ip should get exactly 5
                    assert_eq!(allowed, 5, "ip {i} should have exactly 5 allowed");
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(limiter.len(), 20);
    }

    #[test]
    fn concurrent_cleanup_during_checks() {
        let limiter = Arc::new(RateLimiter::new(TokenBucketConfig {
            capacity: 100,
            refill_rate: 100.0,
        }));

        // spawn checkers
        let check_handles: Vec<_> = (1..=5u8)
            .map(|i| {
                let limiter = Arc::clone(&limiter);
                std::thread::spawn(move || {
                    for _ in 0..50 {
                        let _ = limiter.check(ip(i));
                    }
                })
            })
            .collect();

        // spawn cleaner concurrently
        let cleaner = {
            let limiter = Arc::clone(&limiter);
            std::thread::spawn(move || {
                for _ in 0..10 {
                    limiter.cleanup_stale(Instant::now());
                }
            })
        };

        for h in check_handles {
            h.join().unwrap();
        }
        cleaner.join().unwrap();
        // should not panic — main assertion is no deadlock or crash
    }

    // --- edge cases ---

    #[test]
    fn first_check_for_new_ip_always_allowed() {
        // even with capacity 1, first check should succeed
        let limiter = RateLimiter::new(TokenBucketConfig {
            capacity: 1,
            refill_rate: 0.0,
        });
        for i in 0..=255u8 {
            assert!(
                limiter.check(ip(i)),
                "first check for new ip should always succeed"
            );
        }
    }

    #[test]
    fn new_bucket_starts_at_full_capacity() {
        let limiter = RateLimiter::new(TokenBucketConfig {
            capacity: 5,
            refill_rate: 0.0,
        });
        let t0 = Instant::now();

        // should allow exactly 5
        let allowed = (0..10)
            .filter(|_| limiter.check_at(localhost(), t0))
            .count();
        assert_eq!(allowed, 5);
    }

    #[test]
    fn very_small_time_increment() {
        let limiter = RateLimiter::new(TokenBucketConfig {
            capacity: 1,
            refill_rate: 1.0,
        });
        let t0 = Instant::now();
        assert!(limiter.check_at(localhost(), t0));
        assert!(!limiter.check_at(localhost(), t0));

        // 1 nanosecond later: refill is ~1e-9 tokens, still not enough
        let t_nano = t0 + Duration::from_nanos(1);
        assert!(!limiter.check_at(localhost(), t_nano));
    }

    #[test]
    fn large_capacity() {
        let limiter = RateLimiter::new(TokenBucketConfig {
            capacity: 1_000_000,
            refill_rate: 0.0,
        });
        let t0 = Instant::now();

        for _ in 0..1_000_000 {
            assert!(limiter.check_at(localhost(), t0));
        }
        assert!(!limiter.check_at(localhost(), t0));
    }

    #[test]
    fn default_config_allows_10_then_rejects() {
        let limiter = RateLimiter::new(TokenBucketConfig::default());
        let t0 = Instant::now();

        for i in 0..10 {
            assert!(
                limiter.check_at(localhost(), t0),
                "request {i} should be allowed with default config"
            );
        }
        assert!(
            !limiter.check_at(localhost(), t0),
            "11th request should be rejected with default config"
        );
    }
}
