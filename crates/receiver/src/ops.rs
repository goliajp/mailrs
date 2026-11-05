//! What the receiver tells the ops daemon: its start, that it is ready,
//! and every 30 seconds its counters. These go to the daemon directly, so
//! they keep arriving while kevy-server is down.

use std::sync::OnceLock;
use std::time::Duration;

use mailrs_ops_report::push::Reporter;
use mailrs_ops_report::wire::{Detail, ParsingBeat, ReceiverBeat};

use crate::stats::SharedCounters;

const HEARTBEAT: Duration = Duration::from_secs(30);

static REPORTER: OnceLock<Reporter> = OnceLock::new();

/// Count this start and report it. First thing in [`crate::run`].
pub fn start() {
    REPORTER.get_or_init(|| Reporter::start("receiver"));
}

pub fn ready() {
    if let Some(r) = REPORTER.get() {
        r.ready(None);
    }
}

/// The heartbeat, from the same counters the stats frame carries. Nothing
/// when [`start`] was not called (tests that start a receiver directly).
pub fn spawn_heartbeat(counters: SharedCounters) {
    let Some(r) = REPORTER.get() else {
        return;
    };
    r.spawn_heartbeat(HEARTBEAT, move || Some(beat(&counters)));
}

fn beat(counters: &SharedCounters) -> Detail {
    let s = counters.snapshot();
    let parsing = s.parsing.filter(|p| p.worker).map(|p| ParsingBeat {
        failures_24h: p.failures_24h,
        paused_since: p.paused_since,
    });
    Detail::Receiver(ReceiverBeat {
        active_connections: s.active_connections,
        total_connections: s.total_connections,
        total_messages: s.total_messages,
        parsing,
    })
}
