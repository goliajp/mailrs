//! The spool the receiver writes and a core drains.
//!
//! The receiver accepts SMTP and writes `{spool_root}/incoming/{new,cur}/*`
//! files with the envelope header prepended. A core drains both
//! directories on every tick; a tick runs when the receiver says it spooled
//! a message, or when the interval runs out, whichever is first.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;

use crate::drain::{Core, drain_once};

pub(crate) mod aging;
pub mod attempts;
mod progress;
pub mod quarantine;
pub(crate) mod stuck;
mod wake;

pub use progress::{drain_progress, ops_status};
pub use wake::is_spool_delivered;

/// The two directories a spool is drained from.
#[derive(Clone, Debug)]
pub struct SpoolDirs {
    pub new: PathBuf,
    pub cur: PathBuf,
}

impl SpoolDirs {
    /// `root` is the spool base; the receiver writes under `incoming/`.
    pub fn under(root: &Path) -> Self {
        let incoming = root.join("incoming");
        Self {
            new: incoming.join("new"),
            cur: incoming.join("cur"),
        }
    }
}

/// What a health probe reports about a spool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpoolHealth {
    /// Files set aside in `quarantine/` (see [`quarantine`]).
    pub quarantined: u64,
    /// Age in seconds of the oldest file in `incoming/new/`; 0 when it is
    /// empty. A file that sits there for minutes is one the drain is not
    /// getting through.
    pub oldest_incoming_age_seconds: u64,
}

/// The spool under `root`, as [`SpoolHealth`] reports it: two directory
/// reads.
pub fn health(root: &Path) -> SpoolHealth {
    let now = std::time::SystemTime::now();
    let oldest = std::fs::read_dir(SpoolDirs::under(root).new)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.metadata().and_then(|m| m.modified()).ok())
        .filter_map(|m| now.duration_since(m).ok())
        .map(|age| age.as_secs())
        .max()
        .unwrap_or(0);
    SpoolHealth {
        quarantined: quarantine::count(root),
        oldest_incoming_age_seconds: oldest,
    }
}

/// Drain `new` and then `cur` once. Returns how many files were delivered.
pub fn tick<C: Core + ?Sized>(dirs: &SpoolDirs, core: &C) -> usize {
    let (delivered_new, seen_new) = drain_once(&dirs.new, core);
    let (delivered_cur, seen_cur) = drain_once(&dirs.cur, core);
    let mut seen_all = seen_new;
    seen_all.extend(seen_cur);
    stuck::forget_departed(&seen_all);
    let total = delivered_new + delivered_cur;
    progress::record_tick(total);
    if total > 0 {
        tracing::info!(delivered = total, "spool drain tick");
    }
    total
}

/// Run `drain` forever: now, then whenever a message is spooled or
/// `interval` runs out. `net_url` is the network kevy the receiver
/// announces spooled mail on; without one only the interval wakes it.
///
/// The interval is the latency for mail that arrives while the
/// subscription is down, so it does not back off when idle: an empty tick
/// costs two readdirs on two empty directories.
pub async fn run<F, Fut>(interval: Duration, net_url: Option<String>, mut drain: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ()>,
{
    let wake = Arc::new(Notify::new());
    if let Some(url) = net_url {
        wake::spawn(url, wake.clone());
    }
    loop {
        drain().await;
        tokio::select! {
            _ = tokio::time::sleep(interval) => {}
            _ = wake.notified() => {}
        }
    }
}
