//! Periodic embedded-kevy AOF compaction (2026-07-17 incident remediation).
//!
//! The AOF grew to 2.2 GB in prod, which meant 12-second replays with a
//! transient memory peak above 1 GiB on every boot (the 07-16 OOM restart
//! loop) and a huge blast radius when a torn tail frame black-holed the
//! log (the 07-17 data-loss incident). kevy's built-in auto-rewrite
//! only fires on 100% growth, which a large baseline effectively never
//! reaches. Until kevy ships an absolute-size trigger (feedback filed
//! upstream), compact host-side: check hourly, rewrite when the on-disk AOF
//! exceeds the threshold and has grown by half since the last rewrite.
//!
//! The second half is what lets it settle. The threshold alone said
//! "rewrite whenever the file is over 256 MB", and once the live data
//! itself compacted to more than that (399 MB on 2026-11-02, 536K keys) it
//! rewrote 400 MB to the same 400 MB every hour, 2.6 s each time, for
//! nothing.

use std::sync::Arc;
use std::time::Duration;

use tokio::time::sleep;

use crate::FastcoreState;

const TICK_INTERVAL: Duration = Duration::from_secs(60 * 60);
const INITIAL_DELAY: Duration = Duration::from_secs(5 * 60);
const REWRITE_THRESHOLD_BYTES: u64 = 256 * 1024 * 1024;

/// Spawn the hourly compaction check. Called once from `lib::run`.
pub fn spawn(state: Arc<FastcoreState>, kevy_dir: String) {
    tokio::spawn(async move {
        sleep(INITIAL_DELAY).await;
        // the size the last rewrite left; unknown until one has run
        let mut compacted: Option<u64> = None;
        loop {
            let size = aof_bytes(&kevy_dir);
            if due(size, compacted) {
                let started = std::time::Instant::now();
                match state.mailbox.store_ref().rewrite_aof() {
                    Ok(stats) => {
                        let after = aof_bytes(&kevy_dir);
                        compacted = Some(after);
                        tracing::info!(
                            before_bytes = size,
                            after_bytes = after,
                            elapsed_ms = started.elapsed().as_millis() as u64,
                            stats = ?stats,
                            "aof-compact: rewrote oversized AOF"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(err = %e, before_bytes = size, "aof-compact: rewrite failed");
                    }
                }
            }
            sleep(TICK_INTERVAL).await;
        }
    });
}

/// Whether an AOF of `size` bytes is worth rewriting: over the threshold,
/// and half again as large as the last rewrite left it. With no rewrite
/// yet in this process the file may be anything, so the threshold decides.
fn due(size: u64, compacted: Option<u64>) -> bool {
    size > REWRITE_THRESHOLD_BYTES && compacted.is_none_or(|c| size > c + c / 2)
}

/// Total size of every `aof-*.aof` under the kevy data dir. Missing dir
/// or unreadable entries just count as zero.
fn aof_bytes(dir: &str) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut total = 0u64;
    for e in entries.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("aof-") && name.ends_with(".aof") {
            total += e.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1024 * 1024;

    #[test]
    fn a_large_file_is_rewritten_once_and_then_left_until_it_grows() {
        // first look in this process: the threshold decides
        assert!(due(399 * MB, None));
        assert!(!due(200 * MB, None));
        // the rewrite left it at 399 MB: the live data is that big
        assert!(!due(399 * MB, Some(399 * MB)));
        assert!(!due(590 * MB, Some(399 * MB)));
        assert!(due(600 * MB, Some(399 * MB)));
    }

    #[test]
    fn a_small_store_still_waits_for_the_threshold() {
        assert!(!due(150 * MB, Some(50 * MB)));
        assert!(due(300 * MB, Some(50 * MB)));
    }
}
