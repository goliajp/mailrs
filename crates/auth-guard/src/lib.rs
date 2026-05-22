#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use std::net::IpAddr;
use std::time::{Duration, Instant};

use dashmap::DashMap;

/// Tunable thresholds for the auth-guard's lockout policy.
///
/// Defaults are tuned for an SMTP/IMAP submission server:
/// - 5 failures per (IP, username) in a 15-minute window → 30-minute lockout
/// - 20 failures per IP (any user) in a 60-minute window → 1-hour lockout
/// - Exponential backoff with multiplier 2.0, capped at 24 hours
pub struct AuthGuardConfig {
    /// Failures within `account_window_secs` to trigger an account-level
    /// lockout. Counts per (IP, username) pair.
    pub max_failures_account: u32,
    /// Sliding-window length for counting account-level failures.
    pub account_window_secs: u64,
    /// First-lockout duration for account-level breaches. Subsequent
    /// lockouts grow by `backoff_multiplier` ^ consecutive_lockouts.
    pub base_lockout_secs: u64,
    /// Failures within `ip_window_secs` (any user) to trigger an
    /// IP-level lockout.
    pub max_failures_ip: u32,
    /// Sliding-window length for counting IP-level failures.
    pub ip_window_secs: u64,
    /// First-lockout duration for IP-level breaches.
    pub ip_base_lockout_secs: u64,
    /// Exponential-backoff multiplier applied to repeat offenders.
    /// 2.0 → each subsequent lockout doubles the previous duration.
    pub backoff_multiplier: f64,
    /// Hard ceiling on lockout duration regardless of backoff.
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

/// Result of [`AuthGuard::check`].
pub enum AuthCheck {
    /// No active lockout — the caller should proceed to actually verify
    /// the password.
    Allowed,
    /// Currently locked out; reject the auth attempt without checking
    /// the password. `remaining_secs` is the wall-clock time until
    /// the lockout expires.
    LockedOut {
        /// Seconds until the lockout expires.
        remaining_secs: u64,
    },
}

/// Sharded in-process tracker of failed auth attempts plus lockout
/// state, keyed by `(IpAddr, username)` and by `IpAddr` alone.
///
/// Both counters slide over time windows configured in
/// [`AuthGuardConfig`]. The IP-only counter applies regardless of
/// which username was attempted, so a single attacker spraying many
/// usernames eventually hits the IP-level lockout.
pub struct AuthGuard {
    config: AuthGuardConfig,
    account_failures: DashMap<(IpAddr, String), FailureRecord>,
    ip_failures: DashMap<IpAddr, FailureRecord>,
}

/// Compute the lockout duration with exponential backoff: `base ×
/// multiplier^consecutive_lockouts`, capped at `max_secs`.
///
/// Returns seconds. Equivalent to constructing a [`mailrs_backoff::Backoff`]
/// with `Jitter::None` and reading `base_delay(consecutive_lockouts)`,
/// which is exactly what this function does internally. Lockouts are
/// deterministic by design — you want every offender to see the same
/// penalty — so no jitter.
pub fn lockout_duration(
    base_secs: u64,
    consecutive_lockouts: u32,
    multiplier: f64,
    max_secs: u64,
) -> u64 {
    let backoff = mailrs_backoff::Backoff {
        initial: Duration::from_secs(base_secs),
        multiplier,
        max: Duration::from_secs(max_secs),
        jitter: mailrs_backoff::Jitter::None,
    };
    backoff.base_delay(consecutive_lockouts).as_secs()
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
    /// Construct a guard with the given thresholds. Use
    /// `AuthGuardConfig::default()` for the SMTP/IMAP-tuned defaults.
    pub fn new(config: AuthGuardConfig) -> Self {
        Self {
            config,
            account_failures: DashMap::new(),
            ip_failures: DashMap::new(),
        }
    }

    /// Check whether `(ip, username)` is currently in lockout.
    ///
    /// Read-only; does **not** record an attempt. Call before doing
    /// the actual password verification. If `Allowed`, do the verify;
    /// if `LockedOut`, reject without touching the password backend.
    ///
    /// The check looks at both the per-IP and per-(IP, username)
    /// counters and returns the first matching lockout. IPv6 addresses
    /// are normalized to their /64 prefix.
    pub fn check(&self, ip: IpAddr, username: &str) -> AuthCheck {
        let ip = normalize_ip(ip);
        let now = Instant::now();

        if let Some(rec) = self.ip_failures.get(&ip)
            && let Some(until) = rec.lockout_until
                && now < until {
                    let remaining = until.duration_since(now).as_secs();
                    return AuthCheck::LockedOut {
                        remaining_secs: remaining,
                    };
                }

        let key = (ip, username.to_string());
        if let Some(rec) = self.account_failures.get(&key)
            && let Some(until) = rec.lockout_until
                && now < until {
                    let remaining = until.duration_since(now).as_secs();
                    return AuthCheck::LockedOut {
                        remaining_secs: remaining,
                    };
                }

        AuthCheck::Allowed
    }

    /// Record a failed auth attempt. Call when the password verify
    /// returns "wrong credentials" — including the case where the
    /// account doesn't exist (constant-time policy).
    ///
    /// Increments both the per-IP and per-(IP, username) counters.
    /// May tip one or both over their threshold and arm a lockout.
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

    /// Record a successful auth. Clears the per-(IP, username)
    /// counter (so a legitimate user who fat-fingered then succeeded
    /// doesn't accumulate against future attempts).
    ///
    /// Does **not** clear the per-IP counter, because a successful
    /// auth from one user doesn't prove the IP isn't being abused
    /// against another. Use cleanup_stale + time decay for that.
    pub fn record_success(&self, ip: IpAddr, username: &str) {
        let ip = normalize_ip(ip);
        let key = (ip, username.to_string());
        self.account_failures.remove(&key);
    }

    /// Drop records whose lockouts have already expired before
    /// `before`. Call periodically (every few minutes) from a
    /// background task to keep the maps bounded under sustained
    /// attack volume.
    ///
    /// Records with active lockouts or recent in-window failures
    /// are preserved.
    pub fn cleanup_stale(&self, before: Instant) {
        self.account_failures.retain(|_, rec| {
            if let Some(until) = rec.lockout_until
                && until < before {
                    return false;
                }
            !rec.failures.is_empty() || rec.lockout_until.is_some()
        });
        self.ip_failures.retain(|_, rec| {
            if let Some(until) = rec.lockout_until
                && until < before {
                    return false;
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

    // ===== additional edge cases =====

    #[test]
    fn ipv6_different_subnets_not_blocked_together() {
        let guard = AuthGuard::new(AuthGuardConfig {
            max_failures_account: 2,
            ..Default::default()
        });
        let ip1: IpAddr = "2001:db8:1:2::aaaa".parse().unwrap();
        let ip2: IpAddr = "2001:db8:3:4::bbbb".parse().unwrap(); // different /64
        guard.record_failure(ip1, "alice");
        guard.record_failure(ip1, "alice");
        // ip2 is a different /64 and should NOT be locked
        assert!(matches!(guard.check(ip2, "alice"), AuthCheck::Allowed));
    }

    #[test]
    fn different_usernames_track_independently() {
        let guard = AuthGuard::new(AuthGuardConfig {
            max_failures_account: 2,
            max_failures_ip: 100, // very high so IP-level doesn't trigger
            ..Default::default()
        });
        let ip: IpAddr = "192.0.2.1".parse().unwrap();
        // alice gets 2 failures (at threshold) — should be locked
        guard.record_failure(ip, "alice");
        guard.record_failure(ip, "alice");
        assert!(matches!(guard.check(ip, "alice"), AuthCheck::LockedOut { .. }));
        // bob (same IP, different user) should still be allowed
        assert!(matches!(guard.check(ip, "bob"), AuthCheck::Allowed));
    }

    #[test]
    fn record_failure_during_lockout_does_not_panic() {
        // Once locked out, record_failure can still be called (the
        // attacker keeps probing); the function should not panic.
        let guard = AuthGuard::new(AuthGuardConfig {
            max_failures_account: 2,
            ..Default::default()
        });
        let ip: IpAddr = "192.0.2.10".parse().unwrap();
        guard.record_failure(ip, "alice");
        guard.record_failure(ip, "alice"); // triggers lockout
        // Now keep recording while locked out — must not panic.
        for _ in 0..10 {
            guard.record_failure(ip, "alice");
        }
        // Still locked out (we never cleared)
        assert!(matches!(guard.check(ip, "alice"), AuthCheck::LockedOut { .. }));
    }

    #[test]
    fn record_success_does_not_clear_ip_counter() {
        // Documented contract: record_success clears the per-account
        // counter but NOT the per-IP counter. Verify.
        let guard = AuthGuard::new(AuthGuardConfig {
            max_failures_account: 100,
            max_failures_ip: 3,
            ..Default::default()
        });
        let ip: IpAddr = "192.0.2.20".parse().unwrap();
        // Record 3 IP-level failures with different usernames.
        guard.record_failure(ip, "user1");
        guard.record_failure(ip, "user2");
        guard.record_failure(ip, "user3"); // triggers IP-level lockout
        // user1 succeeds (but it's already too late for the IP)
        guard.record_success(ip, "user1");
        // IP is still locked out for any user
        assert!(matches!(guard.check(ip, "anyone"), AuthCheck::LockedOut { .. }));
    }

    #[test]
    fn cleanup_stale_handles_empty_maps() {
        // cleanup_stale on a fresh guard with no entries should not panic.
        let guard = AuthGuard::new(AuthGuardConfig::default());
        guard.cleanup_stale(Instant::now());
        guard.cleanup_stale(Instant::now() + Duration::from_secs(3600));
    }

    #[test]
    fn zero_max_failures_locks_immediately() {
        // Degenerate config: 1 failure = lockout. (Setting 0 would
        // never lockout because `len >= 0` is always true after the
        // first push, but also tests show >= 1 means "one failure
        // triggers it".)
        let guard = AuthGuard::new(AuthGuardConfig {
            max_failures_account: 1,
            ..Default::default()
        });
        let ip: IpAddr = "192.0.2.30".parse().unwrap();
        guard.record_failure(ip, "alice");
        assert!(matches!(guard.check(ip, "alice"), AuthCheck::LockedOut { .. }));
    }

    #[test]
    fn high_max_lockout_secs_caps_at_max() {
        // Test that lockout_duration is capped at max_lockout_secs
        // even when backoff would otherwise overflow.
        let d = lockout_duration(1800, 100, 2.0, 86400);
        assert_eq!(d, 86400);
    }

    #[test]
    fn backoff_multiplier_below_one_does_not_explode() {
        // 0.5 backoff multiplier — lockout duration should monotonically
        // decrease (not increase) with repeat offenses. Not a useful
        // config but should not panic.
        let d0 = lockout_duration(1800, 0, 0.5, 86400);
        let d1 = lockout_duration(1800, 1, 0.5, 86400);
        let d2 = lockout_duration(1800, 2, 0.5, 86400);
        assert_eq!(d0, 1800);
        assert!(d1 <= d0);
        assert!(d2 <= d1);
    }

    #[test]
    fn concurrent_record_failures_dont_panic() {
        use std::sync::Arc;
        use std::thread;

        let guard = Arc::new(AuthGuard::new(AuthGuardConfig::default()));
        let ip: IpAddr = "192.0.2.40".parse().unwrap();
        let mut handles = vec![];
        for _ in 0..8 {
            let g = guard.clone();
            handles.push(thread::spawn(move || {
                for _ in 0..50 {
                    g.record_failure(ip, "alice");
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        // After 400 concurrent failures, account should be locked
        // (we never panicked or deadlocked — that's the assertion).
        assert!(matches!(guard.check(ip, "alice"), AuthCheck::LockedOut { .. }));
    }

    #[test]
    fn ipv4_loopback_treated_separately_from_ipv6_loopback() {
        // ::1 and 127.0.0.1 are different IP addresses.
        let guard = AuthGuard::new(AuthGuardConfig {
            max_failures_account: 2,
            ..Default::default()
        });
        let v4: IpAddr = "127.0.0.1".parse().unwrap();
        let v6: IpAddr = "::1".parse().unwrap();
        guard.record_failure(v4, "alice");
        guard.record_failure(v4, "alice");
        assert!(matches!(guard.check(v4, "alice"), AuthCheck::LockedOut { .. }));
        // v6 ::1 is independent — should be allowed
        assert!(matches!(guard.check(v6, "alice"), AuthCheck::Allowed));
    }
}
