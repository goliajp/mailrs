//! One login guard for every way in: web, IMAP, POP3, ManageSieve, SMTP
//! submission, DAV, credential checks.
//!
//! Failures are counted over four scopes, each a sliding window:
//!
//! | scope | default | lock |
//! |---|---|---|
//! | source + account | 5 in 15 min | 30 min, doubling |
//! | source (IPv4 /32, IPv6 /64) | 20 in 1 h | 1 h, doubling |
//! | account, any source | 10 in 1 h | 15 min, doubling up to 2 h |
//! | everything | 100 in 10 min | while the window stays full |
//!
//! A source the account signed in from within the last 30 days (IPv4 /24,
//! IPv6 /64, at most 20 kept) is *familiar*, and only the source + account
//! scope applies to it. Someone guessing at an account from elsewhere
//! cannot lock its owner out of the networks the owner uses.
//!
//! The policy is written once against [`Ops`], a handful of key-value
//! primitives, so the in-process [`MemoryLoginGuard`] and a shared-store
//! implementation (the `kevy` feature) make the same decisions. Callers
//! hold an `Arc<dyn LoginGuard>`, ask [`LoginGuard::check`] before looking
//! at the password and report the outcome afterwards; a locked attempt is
//! answered without verifying the password at all.
//!
//! A store that cannot be reached lets the login through and logs an
//! error: a guard that is down for a while is better than nobody being
//! able to sign in.

use std::io;
use std::net::IpAddr;

use async_trait::async_trait;

use crate::alerts::{self, Alert, Kind};
use crate::events::EventOps;

/// One counted scope's threshold and lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Failures within the window that arm the lock.
    pub max_failures: u32,
    /// Window length, seconds.
    pub window_secs: u64,
    /// First lock, seconds; each consecutive lock within a day doubles it.
    pub base_lock_secs: u64,
    /// Longest lock, seconds.
    pub max_lock_secs: u64,
}

/// Thresholds for all four scopes and the familiar-source memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginPolicy {
    /// One source guessing at one account.
    pub source_account: Limits,
    /// One source, any account.
    pub source: Limits,
    /// One account, any source. Applies to unfamiliar sources only.
    pub account: Limits,
    /// Failures across everything that close the door to unfamiliar
    /// sources until the window has room again.
    pub global_max_failures: u32,
    /// Window of the global count, seconds.
    pub global_window_secs: u64,
    /// How long a successful sign-in keeps its source familiar, seconds.
    pub familiar_secs: u64,
    /// Familiar sources kept per account; the oldest go first.
    pub familiar_max: usize,
}

impl Default for LoginPolicy {
    fn default() -> Self {
        Self {
            source_account: Limits {
                max_failures: 5,
                window_secs: 15 * 60,
                base_lock_secs: 30 * 60,
                max_lock_secs: 24 * 3600,
            },
            source: Limits {
                max_failures: 20,
                window_secs: 3600,
                base_lock_secs: 3600,
                max_lock_secs: 24 * 3600,
            },
            account: Limits {
                max_failures: 10,
                window_secs: 3600,
                base_lock_secs: 15 * 60,
                max_lock_secs: 2 * 3600,
            },
            global_max_failures: 100,
            global_window_secs: 10 * 60,
            familiar_secs: 30 * 24 * 3600,
            familiar_max: 20,
        }
    }
}

/// What [`LoginGuard::check`] decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Go on and verify the credentials.
    Allowed,
    /// Refuse without verifying. Which scope tripped is deliberately not
    /// said: the caller shows "too many attempts, try again in N minutes".
    Locked {
        /// Seconds until the attempt is worth making again.
        retry_after_secs: u64,
    },
}

impl Verdict {
    /// Whole minutes to show a person, at least one.
    pub fn retry_after_minutes(&self) -> u64 {
        match self {
            Verdict::Allowed => 0,
            Verdict::Locked { retry_after_secs } => retry_after_secs.div_ceil(60).max(1),
        }
    }
}

/// The guard every login entry point asks.
#[async_trait]
pub trait LoginGuard: Send + Sync {
    /// Before verifying credentials. Records nothing.
    async fn check(&self, source: IpAddr, account: &str) -> Verdict;

    /// The credentials were wrong, or name no account.
    async fn failed(&self, source: IpAddr, account: &str);

    /// The credentials were right. Clears this source's count against
    /// this account and makes the source familiar to it.
    async fn succeeded(&self, source: IpAddr, account: &str);

    /// Keep the fact of an attempt, and log it.
    async fn record(&self, event: crate::events::LoginEvent);

    /// The facts kept, newest first.
    async fn events(
        &self,
        query: crate::events::Query,
    ) -> io::Result<Vec<crate::events::LoginEvent>>;

    /// Up to `max` queued alerts, oldest first, taken off the queue.
    async fn take_alerts(&self, max: usize) -> io::Result<Vec<Alert>>;
}

/// The storage primitives the policy needs. Every key carries its own
/// expiry; nothing has to be swept.
pub trait Ops {
    /// The number stored at `key`.
    fn get(&mut self, key: &str) -> io::Result<Option<u64>>;
    /// Store `value` at `key`, expiring in `ttl_secs`.
    fn set(&mut self, key: &str, value: u64, ttl_secs: u64) -> io::Result<()>;
    /// Add one to the counter `key`, giving it `ttl_secs` when it is new.
    fn incr(&mut self, key: &str, ttl_secs: u64) -> io::Result<u64>;
    /// The counters' values, 0 for absent ones.
    fn counts(&mut self, keys: &[String]) -> io::Result<Vec<u64>>;
    /// Remove `keys`.
    fn del(&mut self, keys: &[String]) -> io::Result<()>;
    /// One field of the hash `key`.
    fn hget(&mut self, key: &str, field: &str) -> io::Result<Option<u64>>;
    /// Every field of the hash `key`.
    fn hgetall(&mut self, key: &str) -> io::Result<Vec<(String, u64)>>;
    /// Set one field and make the whole hash expire in `ttl_secs`.
    fn hset(&mut self, key: &str, field: &str, value: u64, ttl_secs: u64) -> io::Result<()>;
    /// Remove fields of the hash `key`.
    fn hdel(&mut self, key: &str, fields: &[String]) -> io::Result<()>;
}

/// Buckets per window: the window slides in steps of a tenth of itself.
const BUCKETS: u64 = 10;
/// How long a run of consecutive locks is remembered.
const STRIKE_MEMORY_SECS: u64 = 24 * 3600;

/// The four counted scopes of one attempt.
struct Scopes {
    source_account: String,
    source: String,
    account: String,
    familiar_key: String,
    familiar_field: String,
}

impl Scopes {
    fn of(source: IpAddr, account: &str) -> Self {
        let account = normalize_account(account);
        let source = crate::normalize_ip(source);
        Self {
            source_account: format!("sa:{source}:{account}"),
            source: format!("s:{source}"),
            familiar_key: format!("ag:f:{account}"),
            familiar_field: familiar_prefix(source),
            account: format!("a:{account}"),
        }
    }
}

pub mod contract;
mod memory;
mod networks;

pub use memory::{MemoryLoginGuard, MemoryOps};

const GLOBAL: &str = "g";

/// How an account name is kept in a key. Something that is not an address
/// is often a password typed into the wrong field, so only its hash is kept.
fn normalize_account(account: &str) -> String {
    let a = account.trim().to_lowercase();
    if !looks_like_address(&a) {
        use sha2::Digest as _;
        let digest = sha2::Sha256::digest(a.as_bytes());
        let hex: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
        return format!("~{hex}");
    }
    a
}

/// `local@domain.tld` with nothing odd in it.
pub fn looks_like_address(name: &str) -> bool {
    let name = name.trim();
    match name.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !domain.contains('@')
                && name.len() <= 254
                && name.chars().all(|c| c.is_ascii_graphic() || !c.is_ascii())
        }
        None => false,
    }
}

/// An account name as logs may show it: as typed when it is an address,
/// otherwise only its length, since it may well be a password.
pub fn loggable_account(name: &str) -> String {
    match looks_like_address(name) {
        true => name.trim().to_string(),
        false => format!("<{} chars, not an address>", name.chars().count()),
    }
}

/// The network a familiar source stands for: IPv4 /24, IPv6 /64.
pub fn familiar_prefix(source: IpAddr) -> String {
    match crate::normalize_ip(source) {
        IpAddr::V4(v4) => {
            let [a, b, c, _] = v4.octets();
            format!("{a}.{b}.{c}.0/24")
        }
        v6 => format!("{v6}/64"),
    }
}

/// Networks an account signed in from in the last few minutes.
fn recent_key(account_scope: &str) -> String {
    format!("ag:r:{account_scope}")
}

fn lock_key(scope: &str) -> String {
    format!("ag:l:{scope}")
}

fn strike_key(scope: &str) -> String {
    format!("ag:k:{scope}")
}

fn bucket_width(window_secs: u64) -> u64 {
    (window_secs / BUCKETS).max(1)
}

fn bucket_key(scope: &str, index: u64) -> String {
    format!("ag:w:{scope}:{index}")
}

/// The keys holding the window that ends at `now`, current bucket first.
fn window_keys(scope: &str, window_secs: u64, now: u64) -> Vec<String> {
    let current = now / bucket_width(window_secs);
    (0..BUCKETS)
        .filter_map(|back| current.checked_sub(back))
        .map(|i| bucket_key(scope, i))
        .collect()
}

fn window_count<O: Ops>(ops: &mut O, scope: &str, window_secs: u64, now: u64) -> io::Result<u64> {
    Ok(ops
        .counts(&window_keys(scope, window_secs, now))?
        .into_iter()
        .sum())
}

/// Count one failure in `scope`'s window and return the new total.
fn count_failure<O: Ops>(ops: &mut O, scope: &str, window_secs: u64, now: u64) -> io::Result<u64> {
    let width = bucket_width(window_secs);
    ops.incr(&bucket_key(scope, now / width), window_secs + width)?;
    window_count(ops, scope, window_secs, now)
}

/// Lock duration after `strikes` earlier locks within a day.
pub fn lock_secs(limits: &Limits, strikes: u32) -> u64 {
    crate::lockout_duration(limits.base_lock_secs, strikes, 2.0, limits.max_lock_secs)
}

/// The policy's answer before credentials are verified.
pub fn check<O: Ops>(
    ops: &mut O,
    policy: &LoginPolicy,
    source: IpAddr,
    account: &str,
    now: u64,
) -> io::Result<Verdict> {
    let s = Scopes::of(source, account);
    if let Some(left) = lock_left(ops, &s.source_account, now)? {
        return Ok(locked(left));
    }
    if is_familiar(ops, policy, &s, now)? {
        return Ok(Verdict::Allowed);
    }
    for scope in [&s.source, &s.account] {
        if let Some(left) = lock_left(ops, scope, now)? {
            return Ok(locked(left));
        }
    }
    if window_count(ops, GLOBAL, policy.global_window_secs, now)?
        >= u64::from(policy.global_max_failures)
    {
        return Ok(locked(bucket_width(policy.global_window_secs)));
    }
    Ok(Verdict::Allowed)
}

/// Seconds left on `scope`'s lock. A lock stores when it ends, so the
/// caller's clock decides, not the store's.
fn lock_left<O: Ops>(ops: &mut O, scope: &str, now: u64) -> io::Result<Option<u64>> {
    Ok(ops
        .get(&lock_key(scope))?
        .filter(|until| *until > now)
        .map(|until| until - now))
}

fn locked(secs: u64) -> Verdict {
    Verdict::Locked {
        retry_after_secs: secs.max(1),
    }
}

fn is_familiar<O: Ops>(
    ops: &mut O,
    policy: &LoginPolicy,
    s: &Scopes,
    now: u64,
) -> io::Result<bool> {
    Ok(ops
        .hget(&s.familiar_key, &s.familiar_field)?
        .is_some_and(|seen| seen.saturating_add(policy.familiar_secs) > now))
}

/// Record a failed attempt in all four scopes, locking any that crossed
/// its threshold.
pub fn record_failure<O: Ops + EventOps>(
    ops: &mut O,
    policy: &LoginPolicy,
    source: IpAddr,
    account: &str,
    now: u64,
) -> io::Result<()> {
    let s = Scopes::of(source, account);
    for (scope, limits, name) in [
        (&s.source_account, &policy.source_account, "source_account"),
        (&s.source, &policy.source, "source"),
        (&s.account, &policy.account, "account"),
    ] {
        let total = count_failure(ops, scope, limits.window_secs, now)?;
        if total >= u64::from(limits.max_failures) && lock_left(ops, scope, now)?.is_none() {
            let strikes = ops.incr(&strike_key(scope), STRIKE_MEMORY_SECS)?;
            let secs = lock_secs(limits, u32::try_from(strikes - 1).unwrap_or(u32::MAX));
            ops.set(&lock_key(scope), now + secs, secs)?;
            ops.del(&window_keys(scope, limits.window_secs, now))?;
            let account = loggable_account(account);
            tracing::warn!(event = "auth_lockout", scope = name, %source, account, lock_secs = secs);
            // one source against one account is most often the owner
            // mistyping or an old device retrying, so it raises nothing
            let (kind, subject) = match name {
                "source" => (Kind::SourceLocked, source.to_string()),
                "account" => (Kind::AccountLocked, account.clone()),
                _ => continue,
            };
            let detail = format!(
                "{} failed sign-ins within {} minutes (the last from {source}, as {account}); locked for {} minutes",
                limits.max_failures,
                limits.window_secs / 60,
                secs / 60
            );
            alerts::raise(
                ops,
                &Alert {
                    kind,
                    subject,
                    at: now,
                    detail,
                },
            )?;
        }
    }
    let total = count_failure(ops, GLOBAL, policy.global_window_secs, now)?;
    if total == u64::from(policy.global_max_failures) {
        tracing::warn!(event = "auth_lockout", scope = "global", failures = total);
        let detail = format!(
            "{total} failed sign-ins server-wide within {} minutes; sources that never signed in to the account they try are refused until the count drops",
            policy.global_window_secs / 60
        );
        let alert = Alert {
            kind: Kind::GlobalLocked,
            subject: "*".into(),
            at: now,
            detail,
        };
        alerts::raise(ops, &alert)?;
    }
    Ok(())
}

/// Record a successful sign-in.
pub fn record_success<O: Ops + EventOps>(
    ops: &mut O,
    policy: &LoginPolicy,
    source: IpAddr,
    account: &str,
    now: u64,
) -> io::Result<()> {
    let s = Scopes::of(source, account);
    let mut clear = window_keys(&s.source_account, policy.source_account.window_secs, now);
    clear.push(strike_key(&s.source_account));
    ops.del(&clear)?;
    ops.hset(
        &s.familiar_key,
        &s.familiar_field,
        now,
        policy.familiar_secs,
    )?;
    let mut seen = ops.hgetall(&s.familiar_key)?;
    seen.sort_by_key(|(_, at)| std::cmp::Reverse(*at));
    let stale: Vec<String> = seen
        .into_iter()
        .enumerate()
        .filter(|(rank, (_, at))| {
            *rank >= policy.familiar_max || at.saturating_add(policy.familiar_secs) <= now
        })
        .map(|(_, (field, _))| field)
        .collect();
    if !stale.is_empty() {
        ops.hdel(&s.familiar_key, &stale)?;
    }
    networks::many_networks(ops, &s, account, now)
}

/// Unix seconds.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// What a guard does when its store fails: allow, and say so loudly.
pub fn store_unavailable(op: &'static str, error: &io::Error) {
    tracing::error!(event = "auth_guard_unavailable", op, error = %error, "login guard store unreachable; letting the attempt through");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_ops_meet_the_contract() {
        contract::check_ops(MemoryOps::default, |o, now| {
            o.at(now);
        });
    }

    #[test]
    fn familiar_prefixes() {
        assert_eq!(
            familiar_prefix("203.0.113.77".parse().unwrap()),
            "203.0.113.0/24"
        );
        assert_eq!(
            familiar_prefix("2001:db8:1:2:3::4".parse().unwrap()),
            "2001:db8:1:2::/64"
        );
    }

    #[test]
    fn minutes_round_up() {
        assert_eq!(
            Verdict::Locked {
                retry_after_secs: 61
            }
            .retry_after_minutes(),
            2
        );
        assert_eq!(
            Verdict::Locked {
                retry_after_secs: 1
            }
            .retry_after_minutes(),
            1
        );
    }

    #[test]
    fn overlong_names_are_kept_as_a_hash() {
        let long = format!("{}@x.test", "a".repeat(5000));
        let kept = normalize_account(&long);
        assert!(kept.starts_with('~') && kept.len() == 33, "{kept}");
    }

    #[test]
    fn names_that_are_not_addresses_are_neither_kept_nor_logged() {
        assert!(looks_like_address("Alice@X.test"));
        for not in [
            "hunter2",
            "alice",
            "alice@",
            "@x.test",
            "alice@localhost",
            "a b@x.test",
            "a@x.test.",
            "",
        ] {
            assert!(!looks_like_address(not), "{not}");
        }
        let kept = normalize_account("Tr0ub4dor&3");
        assert!(
            kept.starts_with('~') && !kept.contains("tr0ub4dor"),
            "{kept}"
        );
        assert_eq!(
            normalize_account("Tr0ub4dor&3"),
            kept,
            "the same input, the same key"
        );
        assert_eq!(normalize_account(" Alice@X.test "), "alice@x.test");
        assert_eq!(
            loggable_account("Tr0ub4dor&3"),
            "<11 chars, not an address>"
        );
        assert_eq!(loggable_account("alice@x.test"), "alice@x.test");
    }

    #[tokio::test]
    async fn memory_guard_through_the_trait() {
        let g: std::sync::Arc<dyn LoginGuard> = std::sync::Arc::new(MemoryLoginGuard::default());
        let ip: IpAddr = "192.0.2.1".parse().unwrap();
        for _ in 0..5 {
            assert_eq!(g.check(ip, "a@x.test").await, Verdict::Allowed);
            g.failed(ip, "a@x.test").await;
        }
        assert!(matches!(
            g.check(ip, "a@x.test").await,
            Verdict::Locked { .. }
        ));
    }
}
