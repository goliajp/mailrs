//! Receiver-process counters, and the frame that carries them to whoever
//! is rendering the SMTP live monitor.
//!
//! These live here rather than in the prometheus recorder because the
//! monitor is a different consumer with a different shape: it wants four
//! current numbers over a socket, not a scrape endpoint. The prometheus
//! counters stay exactly as they were — this records alongside them.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub use mailrs_core_sidestate::smtp_monitor::ReceiverStats;

/// Process-lifetime counters for one receiver.
pub struct ReceiverCounters {
    active: AtomicU64,
    connections: AtomicU64,
    messages: AtomicU64,
    started: Instant,
}

impl Default for ReceiverCounters {
    fn default() -> Self {
        Self {
            active: AtomicU64::new(0),
            connections: AtomicU64::new(0),
            messages: AtomicU64::new(0),
            started: Instant::now(),
        }
    }
}

impl ReceiverCounters {
    pub fn on_connect(&self) {
        self.active.fetch_add(1, Ordering::Relaxed);
        self.connections.fetch_add(1, Ordering::Relaxed);
    }

    /// Saturating, deliberately. There are thirteen return paths that
    /// close a connection against two that open one; if any of them ever
    /// counts twice, a wrapping decrement would report eighteen
    /// quintillion active sessions rather than one too few.
    pub fn on_disconnect(&self) {
        let _ = self
            .active
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                Some(v.saturating_sub(1))
            });
    }

    pub fn on_message(&self) {
        self.messages.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> ReceiverStats {
        ReceiverStats {
            active_connections: self.active.load(Ordering::Relaxed),
            total_connections: self.connections.load(Ordering::Relaxed),
            total_messages: self.messages.load(Ordering::Relaxed),
            uptime_secs: self.started.elapsed().as_secs(),
        }
    }
}

pub type SharedCounters = Arc<ReceiverCounters>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_paired_session_returns_the_gauge_to_zero() {
        let c = ReceiverCounters::default();
        c.on_connect();
        assert_eq!(c.snapshot().active_connections, 1);
        c.on_disconnect();
        assert_eq!(c.snapshot().active_connections, 0);
        // …and the lifetime total does not go back down with it.
        assert_eq!(c.snapshot().total_connections, 1);
    }

    /// The load-bearing one: an unpaired close must not wrap. Without
    /// the saturating update this reads `u64::MAX`, which renders as a
    /// twenty-digit "active connections" and is worse than being wrong
    /// by one.
    #[test]
    fn an_unpaired_close_does_not_wrap() {
        let c = ReceiverCounters::default();
        c.on_disconnect();
        assert_eq!(c.snapshot().active_connections, 0);
    }

    #[test]
    fn messages_and_connections_count_separately() {
        let c = ReceiverCounters::default();
        c.on_connect();
        c.on_message();
        c.on_message();
        let s = c.snapshot();
        assert_eq!(s.total_connections, 1);
        assert_eq!(s.total_messages, 2);
    }
}
