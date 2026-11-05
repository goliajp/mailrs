//! What a mailrs process tells the ops daemon about itself, and how.
//!
//! Every long-running process pushes small signed reports to the daemon's
//! `POST /v1/reports`: one when it starts, one when it is ready, and, for
//! the processes the daemon cannot poll over HTTP, a heartbeat. The daemon
//! never connects to kevy-server, so these reports are how it sees those
//! processes at all.
//!
//! The format lives here so the process and the daemon import the same
//! types: a field written on one side and not read on the other is a
//! compile error, not a silent gap.
//!
//! - [`wire`]: the report and each process's own fields
//! - [`key`]: per-process signing keys derived from one master key
//! - [`counter`]: the start counter and instance id in the process's own
//!   data directory, and the liveness file healthchecks read
//! - [`build`]: version, commit and start time of the running binary
//! - `push` (feature `push`): the best-effort sender

pub mod build;
pub mod counter;
pub mod key;
pub mod wire;

#[cfg(feature = "push")]
pub mod push;

use std::sync::atomic::{AtomicI64, Ordering};

static KEVY_CONTACT_AT: AtomicI64 = AtomicI64::new(0);

/// Record that this process just completed an operation on kevy-server.
/// Called from each process's kevy connection wrapper.
pub fn kevy_contact() {
    KEVY_CONTACT_AT.store(build::now(), Ordering::Relaxed);
}

/// Unix seconds of the last completed kevy-server operation; `None` when
/// there has been none since start.
pub fn kevy_contact_at() -> Option<i64> {
    match KEVY_CONTACT_AT.load(Ordering::Relaxed) {
        0 => None,
        t => Some(t),
    }
}
