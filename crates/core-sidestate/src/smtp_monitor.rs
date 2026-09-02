//! The SMTP live monitor's wire contract: two pub/sub channel names and
//! the counter frame.
//!
//! It lives here for the reason `sieve_key` does — the writer is
//! `mailrs-receiver` and the reader is `mailrs-webapi`, they share no
//! other crate, and a channel name spelled twice is a channel name that
//! will eventually be spelled two ways. That failure is silent on both
//! sides: the publisher publishes to nobody and the subscriber waits on
//! a channel nothing writes, which is indistinguishable from an idle
//! mail server.
//!
//! Deliberately **pub/sub, not the change feed**. A protocol trace is
//! diagnostic and only wanted while somebody has the monitor open; at
//! roughly a hundred connections an hour times a dozen command lines
//! each, putting it in the durable feed would write tens of thousands of
//! AOF entries a day that no reader ever asks for.

use serde::{Deserialize, Serialize};

/// One SMTP session's protocol trace: `ConnectionOpened` through
/// `ConnectionClosed`, serialised as `SmtpEvent` inside the receiver's
/// notify envelope.
pub const TRACE_CHANNEL: &[u8] = b"trace:smtp";

/// The receiver's four counters. A separate channel so a reader tells a
/// stats frame from a trace frame by the channel it arrived on, rather
/// than by a discriminator inside the payload.
pub const STATS_CHANNEL: &[u8] = b"trace:smtp-stats";

/// What the monitor's four cards render.
///
/// Absent is a real state and means *no receiver has published yet* — the
/// reader shows a dash for it, never a zero. `0` and "unknown" are
/// different answers, and only one of them is true of a mail server that
/// has just started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiverStats {
    pub active_connections: u64,
    pub total_connections: u64,
    pub total_messages: u64,
    pub uptime_secs: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The round trip the two processes actually perform. If a field is
    /// renamed on one side this is what fails, rather than the monitor
    /// quietly showing dashes.
    #[test]
    fn stats_round_trip_over_the_wire() {
        let sent = ReceiverStats {
            active_connections: 2,
            total_connections: 483,
            total_messages: 17,
            uptime_secs: 18_346,
        };
        let bytes = serde_json::to_vec(&sent).unwrap();
        let got: ReceiverStats = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(sent, got);
    }

    /// The channel names are the contract; pin them so a rename has to be
    /// deliberate on both sides at once.
    #[test]
    fn the_channel_names_are_pinned() {
        assert_eq!(TRACE_CHANNEL, b"trace:smtp");
        assert_eq!(STATS_CHANNEL, b"trace:smtp-stats");
        assert_ne!(TRACE_CHANNEL, STATS_CHANNEL);
    }
}
