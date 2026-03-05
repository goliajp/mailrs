use std::net::IpAddr;
use std::time::{Duration, Instant};

use dashmap::DashMap;

pub struct AuthGuardConfig {
    pub max_failures_account: u32,
    pub account_window_secs: u64,
    pub base_lockout_secs: u64,
    pub max_failures_ip: u32,
    pub ip_window_secs: u64,
    pub ip_base_lockout_secs: u64,
    pub backoff_multiplier: f64,
    pub max_lockout_secs: u64,
}

impl Default for AuthGuardConfig {
    fn default() -> Self {
        Self {
            max_failures_account: 5,
            account_window_secs: 900,
            base_lockout_secs: 1800,
            max_failures_ip: 20,
            ip_window_secs: 3600,
            ip_base_lockout_secs: 3600,
            backoff_multiplier: 2.0,
            max_lockout_secs: 86400,
        }
    }
}

struct FailureRecord {
    failures: Vec<Instant>,
    lockout_until: Option<Instant>,
    consecutive_lockouts: u32,
}

pub enum AuthCheck {
    Allowed,
    LockedOut { remaining_secs: u64 },
}

pub struct AuthGuard {
    config: AuthGuardConfig,
    account_failures: DashMap<(IpAddr, String), FailureRecord>,
    ip_failures: DashMap<IpAddr, FailureRecord>,
}

/// compute lockout duration with exponential backoff
pub fn lockout_duration(
    base_secs: u64,
    consecutive_lockouts: u32,
    multiplier: f64,
    max_secs: u64,
) -> u64 {
    let duration = (base_secs as f64 * multiplier.powi(consecutive_lockouts as i32)) as u64;
    duration.min(max_secs)
}

/// normalize IPv6 to /64 prefix for rate limiting
fn normalize_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => {
            let segments = v6.segments();
            let masked = std::net::Ipv6Addr::new(
                segments[0],
                segments[1],
                segments[2],
                segments[3],
                0,
                0,
                0,
                0,
            );
            IpAddr::V6(masked)
        }
        ip => ip,
    }
}

impl AuthGuard {
    pub fn new(config: AuthGuardConfig) -> Self {
        Self {
            config,
            account_failures: DashMap::new(),
            ip_failures: DashMap::new(),
        }
    }

    pub fn check(&self, ip: IpAddr, username: &str) -> AuthCheck {
        let ip = normalize_ip(ip);
        let now = Instant::now();

        if let Some(rec) = self.ip_failures.get(&ip) {
            if let Some(until) = rec.lockout_until {
                if now < until {
                    let remaining = until.duration_since(now).as_secs();
                    return AuthCheck::LockedOut {
                        remaining_secs: remaining,
                    };
                }
            }
        }

        let key = (ip, username.to_string());
        if let Some(rec) = self.account_failures.get(&key) {
            if let Some(until) = rec.lockout_until {
                if now < until {
                    let remaining = until.duration_since(now).as_secs();
                    return AuthCheck::LockedOut {
                        remaining_secs: remaining,
                    };
                }
            }
        }

        AuthCheck::Allowed
    }

    pub fn record_failure(&self, ip: IpAddr, username: &str) {
        let ip = normalize_ip(ip);
        let now = Instant::now();

        tracing::warn!(
            event = "auth_failure",
            ip = %ip,
            username = username,
        );

        // per-(IP, username) tracking
        let key = (ip, username.to_string());
        let mut entry = self
            .account_failures
            .entry(key)
            .or_insert_with(|| FailureRecord {
                failures: Vec::new(),
                lockout_until: None,
                consecutive_lockouts: 0,
            });

        let window_start = now - Duration::from_secs(self.config.account_window_secs);
        entry.failures.retain(|t| *t > window_start);
        entry.failures.push(now);

        if entry.failures.len() as u32 >= self.config.max_failures_account {
            let duration = lockout_duration(
                self.config.base_lockout_secs,
                entry.consecutive_lockouts,
                self.config.backoff_multiplier,
                self.config.max_lockout_secs,
            );
            entry.lockout_until = Some(now + Duration::from_secs(duration));
            entry.consecutive_lockouts += 1;
            entry.failures.clear();

            tracing::warn!(
                event = "auth_lockout",
                ip = %ip,
                username = username,
                scope = "account",
                duration_secs = duration,
            );
        }

        // per-IP tracking
        let mut entry = self.ip_failures.entry(ip).or_insert_with(|| FailureRecord {
            failures: Vec::new(),
            lockout_until: None,
            consecutive_lockouts: 0,
        });

        let window_start = now - Duration::from_secs(self.config.ip_window_secs);
        entry.failures.retain(|t| *t > window_start);
        entry.failures.push(now);

        if entry.failures.len() as u32 >= self.config.max_failures_ip {
            let duration = lockout_duration(
                self.config.ip_base_lockout_secs,
                entry.consecutive_lockouts,
                self.config.backoff_multiplier,
                self.config.max_lockout_secs,
            );
            entry.lockout_until = Some(now + Duration::from_secs(duration));
            entry.consecutive_lockouts += 1;
            entry.failures.clear();

            tracing::warn!(
                event = "auth_lockout",
                ip = %ip,
                scope = "ip",
                duration_secs = duration,
            );
        }
    }

    pub fn record_success(&self, ip: IpAddr, username: &str) {
        let ip = normalize_ip(ip);
        let key = (ip, username.to_string());
        self.account_failures.remove(&key);
    }

    pub fn cleanup_stale(&self, before: Instant) {
        self.account_failures.retain(|_, rec| {
            if let Some(until) = rec.lockout_until {
                if until < before {
                    return false;
                }
            }
            !rec.failures.is_empty() || rec.lockout_until.is_some()
        });
        self.ip_failures.retain(|_, rec| {
            if let Some(until) = rec.lockout_until {
                if until < before {
                    return false;
                }
            }
            !rec.failures.is_empty() || rec.lockout_until.is_some()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lockout_duration_base() {
        assert_eq!(lockout_duration(1800, 0, 2.0, 86400), 1800);
    }

    #[test]
    fn lockout_duration_exponential() {
        assert_eq!(lockout_duration(1800, 2, 2.0, 86400), 7200);
    }

    #[test]
    fn lockout_duration_capped() {
        assert_eq!(lockout_duration(1800, 10, 2.0, 86400), 86400);
    }

    #[test]
    fn allowed_below_threshold() {
        let guard = AuthGuard::new(AuthGuardConfig {
            max_failures_account: 5,
            ..Default::default()
        });
        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        for _ in 0..4 {
            guard.record_failure(ip, "alice");
        }
        assert!(matches!(guard.check(ip, "alice"), AuthCheck::Allowed));
    }

    #[test]
    fn locked_at_threshold() {
        let guard = AuthGuard::new(AuthGuardConfig {
            max_failures_account: 5,
            ..Default::default()
        });
        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        for _ in 0..5 {
            guard.record_failure(ip, "alice");
        }
        assert!(matches!(
            guard.check(ip, "alice"),
            AuthCheck::LockedOut { .. }
        ));
    }

    #[test]
    fn success_resets_account() {
        let guard = AuthGuard::new(AuthGuardConfig {
            max_failures_account: 5,
            ..Default::default()
        });
        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        for _ in 0..4 {
            guard.record_failure(ip, "alice");
        }
        guard.record_success(ip, "alice");
        // should be back to 0 failures
        guard.record_failure(ip, "alice");
        assert!(matches!(guard.check(ip, "alice"), AuthCheck::Allowed));
    }

    #[test]
    fn ipv6_normalized_to_64() {
        let ip1: IpAddr = "2001:db8::1".parse().unwrap();
        let ip2: IpAddr = "2001:db8::ffff".parse().unwrap();
        assert_eq!(normalize_ip(ip1), normalize_ip(ip2));
    }

    #[test]
    fn ipv4_unchanged() {
        let ip: IpAddr = "192.168.1.1".parse().unwrap();
        assert_eq!(normalize_ip(ip), ip);
    }

    #[test]
    fn ipv6_different_subnets_not_merged() {
        let ip1: IpAddr = "2001:db8:aaaa:bbbb::1".parse().unwrap();
        let ip2: IpAddr = "2001:db8:cccc:dddd::1".parse().unwrap();
        assert_ne!(normalize_ip(ip1), normalize_ip(ip2));
    }

    #[test]
    fn ip_lockout_at_threshold() {
        let guard = AuthGuard::new(AuthGuardConfig {
            max_failures_ip: 3,
            max_failures_account: 100, // high so account lock doesn't trigger
            ..Default::default()
        });
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        for _ in 0..3 {
            guard.record_failure(ip, "user1");
        }
        assert!(matches!(
            guard.check(ip, "any_user"),
            AuthCheck::LockedOut { .. }
        ));
    }

    #[test]
    fn lockout_expires_after_duration() {
        // use a very short lockout so we can simulate expiry via Instant arithmetic
        // since Instant doesn't let us go forward, we test via cleanup_stale + re-check pattern
        // instead we directly verify the lockout_until field is in the future
        let guard = AuthGuard::new(AuthGuardConfig {
            max_failures_account: 2,
            base_lockout_secs: 1,
            max_lockout_secs: 1,
            backoff_multiplier: 1.0,
            ..Default::default()
        });
        let ip: IpAddr = "127.0.0.1".parse().unwrap();

        // trigger lockout
        guard.record_failure(ip, "bob");
        guard.record_failure(ip, "bob");
        assert!(matches!(
            guard.check(ip, "bob"),
            AuthCheck::LockedOut { remaining_secs }
            if remaining_secs <= 1
        ));

        // wait just over 1 second for the lockout to expire
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert!(matches!(guard.check(ip, "bob"), AuthCheck::Allowed));
    }

    #[test]
    fn cleanup_stale_removes_expired_lockouts() {
        let guard = AuthGuard::new(AuthGuardConfig {
            max_failures_account: 2,
            base_lockout_secs: 1,
            max_lockout_secs: 1,
            backoff_multiplier: 1.0,
            max_failures_ip: 2,
            ip_base_lockout_secs: 1,
            ..Default::default()
        });
        let ip: IpAddr = "127.0.0.1".parse().unwrap();

        // trigger both account and ip lockout
        guard.record_failure(ip, "carol");
        guard.record_failure(ip, "carol");
        assert!(!guard.account_failures.is_empty());
        assert!(!guard.ip_failures.is_empty());

        // cleanup with a time far in the future should remove everything
        let future = Instant::now() + std::time::Duration::from_secs(3600);
        guard.cleanup_stale(future);
        assert!(guard.account_failures.is_empty());
        assert!(guard.ip_failures.is_empty());
    }

    #[test]
    fn cleanup_stale_preserves_active_records() {
        let guard = AuthGuard::new(AuthGuardConfig {
            max_failures_account: 10,
            max_failures_ip: 10,
            ..Default::default()
        });
        let ip: IpAddr = "127.0.0.1".parse().unwrap();

        // record one failure (no lockout yet, but failures vec is non-empty)
        guard.record_failure(ip, "dave");
        assert_eq!(guard.account_failures.len(), 1);
        assert_eq!(guard.ip_failures.len(), 1);

        // cleanup with current time should keep records (they have recent failures)
        guard.cleanup_stale(Instant::now());
        assert_eq!(guard.account_failures.len(), 1);
        assert_eq!(guard.ip_failures.len(), 1);
    }

    #[test]
    fn normal_login_not_blocked() {
        let guard = AuthGuard::new(AuthGuardConfig::default());
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        // fresh guard, no failures recorded
        assert!(matches!(guard.check(ip, "admin"), AuthCheck::Allowed));
    }

    #[test]
    fn exponential_backoff_increases_lockout() {
        let guard = AuthGuard::new(AuthGuardConfig {
            max_failures_account: 1,
            base_lockout_secs: 10,
            backoff_multiplier: 2.0,
            max_lockout_secs: 86400,
            account_window_secs: 1, // short window so failures don't pile up across lockouts
            ..Default::default()
        });
        let ip: IpAddr = "127.0.0.1".parse().unwrap();

        // first lockout: base = 10s
        guard.record_failure(ip, "eve");
        if let AuthCheck::LockedOut { remaining_secs } = guard.check(ip, "eve") {
            assert!(remaining_secs <= 10);
        } else {
            panic!("expected lockout after first failure");
        }

        // wait for lockout to expire, then trigger second lockout
        std::thread::sleep(std::time::Duration::from_millis(100));
        // manually clear lockout to simulate time passing
        if let Some(mut rec) = guard.account_failures.get_mut(&(ip, "eve".to_string())) {
            rec.lockout_until = None;
        }

        // second lockout: base * 2^1 = 20s
        guard.record_failure(ip, "eve");
        if let AuthCheck::LockedOut { remaining_secs } = guard.check(ip, "eve") {
            assert!(
                remaining_secs > 10,
                "second lockout should be longer than first, got {remaining_secs}"
            );
        } else {
            panic!("expected lockout after second round of failures");
        }
    }

    #[test]
    fn ipv6_lockout_applies_to_same_subnet() {
        let guard = AuthGuard::new(AuthGuardConfig {
            max_failures_account: 2,
            ..Default::default()
        });
        // two different hosts in the same /64
        let ip1: IpAddr = "2001:db8:1:2::aaaa".parse().unwrap();
        let ip2: IpAddr = "2001:db8:1:2::bbbb".parse().unwrap();

        // failures from ip1
        guard.record_failure(ip1, "frank");
        guard.record_failure(ip1, "frank");

        // check from ip2 (same /64) should be locked
        assert!(matches!(
            guard.check(ip2, "frank"),
            AuthCheck::LockedOut { .. }
        ));
    }
}
