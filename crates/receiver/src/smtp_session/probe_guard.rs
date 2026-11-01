//! Slows down guessing which local addresses exist.
//!
//! A refusal at RCPT tells the sender the address reaches nobody, so a
//! client can walk a list of guesses and keep the ones that were accepted.
//! Masked addresses make that worth stopping: their only secret is the
//! random part. Each IP gets [`LIMIT`] unknown recipients per [`WINDOW`];
//! past that every RCPT to a local address is answered `421` before
//! anyone is asked, so the answer no longer depends on whether the
//! address exists.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const LIMIT: u32 = 10;
pub const WINDOW: Duration = Duration::from_secs(3600);

#[derive(Default)]
pub struct ProbeGuard {
    seen: Mutex<HashMap<IpAddr, (Instant, u32)>>,
}

impl ProbeGuard {
    pub fn is_blocked(&self, ip: IpAddr, now: Instant) -> bool {
        let seen = self.seen.lock().expect("probe guard lock");
        seen.get(&ip)
            .is_some_and(|(start, n)| now.duration_since(*start) < WINDOW && *n >= LIMIT)
    }

    pub fn record_unknown(&self, ip: IpAddr, now: Instant) {
        let mut seen = self.seen.lock().expect("probe guard lock");
        let entry = seen.entry(ip).or_insert((now, 0));
        if now.duration_since(entry.0) >= WINDOW {
            *entry = (now, 0);
        }
        entry.1 += 1;
    }

    /// Forget IPs whose window has passed.
    pub fn prune(&self, now: Instant) {
        let mut seen = self.seen.lock().expect("probe guard lock");
        seen.retain(|_, (start, _)| now.duration_since(*start) < WINDOW);
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
            g.record_unknown(A, t);
        }
        assert!(!g.is_blocked(A, t));
        g.record_unknown(A, t);
        assert!(g.is_blocked(A, t));
        assert!(!g.is_blocked(B, t), "another IP keeps its own count");
    }

    #[test]
    fn the_block_lifts_when_the_window_passes() {
        let g = ProbeGuard::default();
        let t = Instant::now();
        for _ in 0..LIMIT {
            g.record_unknown(A, t);
        }
        let later = t + WINDOW;
        assert!(!g.is_blocked(A, later));
        g.record_unknown(A, later);
        assert!(!g.is_blocked(A, later), "a new window starts from one");
    }

    #[test]
    fn prune_drops_only_expired_ips() {
        let g = ProbeGuard::default();
        let t = Instant::now();
        g.record_unknown(A, t);
        g.record_unknown(B, t + Duration::from_secs(1800));
        g.prune(t + WINDOW);
        assert_eq!(g.len(), 1);
        assert!(g.seen.lock().unwrap().contains_key(&B));
    }
}
