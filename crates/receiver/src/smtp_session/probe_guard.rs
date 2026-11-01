//! Slows down guessing which local addresses exist.
//!
//! A refusal at RCPT tells the sender the address reaches nobody, so a
//! client can walk a list of guesses and keep the ones that were accepted.
//! Masked addresses make that worth stopping: their only secret is the
//! random part. Unknown local recipients are counted per [`Key`] over
//! [`WINDOW`]; past the key's limit every RCPT to a local address is
//! answered `421` before anyone is asked, so the answer no longer depends
//! on whether the address exists.
//!
//! Who is counted: a signed-in session by its account ([`ACCOUNT_LIMIT`]);
//! anyone else by IP and MAIL FROM domain when that domain's SPF passes
//! for the IP, so one sender cannot use up the count of others behind the
//! same address, and by IP alone otherwise ([`LIMIT`]).

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const LIMIT: u32 = 10;
pub const ACCOUNT_LIMIT: u32 = 30;
pub const WINDOW: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    Ip(IpAddr),
    /// The MAIL FROM domain, lowercased, whose SPF passed for the IP.
    IpDomain(IpAddr, String),
    /// A signed-in account.
    Account(String),
}

impl Key {
    pub fn limit(&self) -> u32 {
        match self {
            Key::Account(_) => ACCOUNT_LIMIT,
            Key::Ip(_) | Key::IpDomain(..) => LIMIT,
        }
    }

    /// The name the log gives this kind of key.
    pub fn kind(&self) -> &'static str {
        match self {
            Key::Ip(_) => "ip",
            Key::IpDomain(..) => "ip_domain",
            Key::Account(_) => "account",
        }
    }

    pub fn domain(&self) -> Option<&str> {
        match self {
            Key::IpDomain(_, d) => Some(d),
            Key::Ip(_) | Key::Account(_) => None,
        }
    }
}

#[derive(Default)]
pub struct ProbeGuard {
    seen: Mutex<HashMap<Key, (Instant, u32)>>,
    /// SPF verdicts by (IP, domain), so a session asks DNS once an hour
    /// rather than at every RCPT.
    spf: Mutex<HashMap<(IpAddr, String), (Instant, bool)>>,
}

impl ProbeGuard {
    /// Unknown recipients counted for `key` in its current window.
    pub fn count(&self, key: &Key, now: Instant) -> u32 {
        let seen = self.seen.lock().expect("probe guard lock");
        match seen.get(key) {
            Some((start, n)) if now.duration_since(*start) < WINDOW => *n,
            _ => 0,
        }
    }

    pub fn is_blocked(&self, key: &Key, now: Instant) -> bool {
        self.count(key, now) >= key.limit()
    }

    /// Count one more; returns the count after it.
    pub fn record_unknown(&self, key: &Key, now: Instant) -> u32 {
        let mut seen = self.seen.lock().expect("probe guard lock");
        let entry = seen.entry(key.clone()).or_insert((now, 0));
        if now.duration_since(entry.0) >= WINDOW {
            *entry = (now, 0);
        }
        entry.1 += 1;
        entry.1
    }

    pub fn spf_seen(&self, ip: IpAddr, domain: &str, now: Instant) -> Option<bool> {
        let spf = self.spf.lock().expect("probe guard lock");
        spf.get(&(ip, domain.to_string()))
            .filter(|(at, _)| now.duration_since(*at) < WINDOW)
            .map(|(_, pass)| *pass)
    }

    pub fn remember_spf(&self, ip: IpAddr, domain: &str, pass: bool, now: Instant) {
        let mut spf = self.spf.lock().expect("probe guard lock");
        spf.insert((ip, domain.to_string()), (now, pass));
    }

    /// Forget keys and SPF verdicts whose window has passed.
    pub fn prune(&self, now: Instant) {
        let mut seen = self.seen.lock().expect("probe guard lock");
        seen.retain(|_, (start, _)| now.duration_since(*start) < WINDOW);
        let mut spf = self.spf.lock().expect("probe guard lock");
        spf.retain(|_, (at, _)| now.duration_since(*at) < WINDOW);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.seen.lock().expect("probe guard lock").len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(192, 0, 2, 1));
    const B: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(192, 0, 2, 2));

    #[test]
    fn the_eleventh_guess_in_an_hour_is_blocked() {
        let g = ProbeGuard::default();
        let t = Instant::now();
        for _ in 0..LIMIT - 1 {
            g.record_unknown(&Key::Ip(A), t);
        }
        assert!(!g.is_blocked(&Key::Ip(A), t));
        assert_eq!(g.record_unknown(&Key::Ip(A), t), LIMIT);
        assert!(g.is_blocked(&Key::Ip(A), t));
        assert!(
            !g.is_blocked(&Key::Ip(B), t),
            "another IP keeps its own count"
        );
    }

    #[test]
    fn a_domain_and_an_account_are_counted_apart_from_the_ip() {
        let g = ProbeGuard::default();
        let t = Instant::now();
        let shop = Key::IpDomain(A, "shop.example".into());
        for _ in 0..LIMIT {
            g.record_unknown(&shop, t);
        }
        assert!(g.is_blocked(&shop, t));
        assert!(!g.is_blocked(&Key::Ip(A), t));
        assert!(!g.is_blocked(&Key::IpDomain(A, "news.example".into()), t));
        let me = Key::Account("me@x.jp".into());
        for _ in 0..LIMIT {
            g.record_unknown(&me, t);
        }
        assert!(!g.is_blocked(&me, t), "an account has the higher limit");
        for _ in LIMIT..ACCOUNT_LIMIT {
            g.record_unknown(&me, t);
        }
        assert!(g.is_blocked(&me, t));
    }

    #[test]
    fn the_block_lifts_when_the_window_passes() {
        let g = ProbeGuard::default();
        let t = Instant::now();
        for _ in 0..LIMIT {
            g.record_unknown(&Key::Ip(A), t);
        }
        let later = t + WINDOW;
        assert!(!g.is_blocked(&Key::Ip(A), later));
        assert_eq!(g.record_unknown(&Key::Ip(A), later), 1, "a new window");
    }

    #[test]
    fn prune_drops_only_expired_entries() {
        let g = ProbeGuard::default();
        let t = Instant::now();
        g.record_unknown(&Key::Ip(A), t);
        g.record_unknown(&Key::Ip(B), t + Duration::from_secs(1800));
        g.remember_spf(A, "x.example", true, t);
        g.prune(t + WINDOW);
        assert_eq!(g.len(), 1);
        assert_eq!(g.count(&Key::Ip(B), t + WINDOW), 1);
        assert_eq!(g.spf_seen(A, "x.example", t + WINDOW), None);
    }
}
