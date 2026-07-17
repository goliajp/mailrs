//! Periodic embedded-kevy AOF compaction (2026-07-17 incident remediation).
//!
//! The AOF grew to 2.2 GB in prod, which meant 12-second replays with a
//! transient memory peak above 1 GiB on every boot (the 07-16 OOM restart
//! loop) and a huge blast radius when a torn tail frame black-holed the
//! log (the 07-17 data-loss incident). kevy's built-in auto-rewrite
//! only fires on 100% growth, which a large baseline effectively never
//! reaches. Until kevy ships an absolute-size trigger (feedback filed:
//! `.claude/notes/kevy-feedback-aof-blackhole-2026-07-17.md` §3.1),
//! compact host-side: check hourly, rewrite when the on-disk AOF
//! exceeds the threshold. A rewrite of ~120K keys measured ~1 s and is
//! fully online, so the hourly check is effectively free.

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
        loop {
            let size = aof_bytes(&kevy_dir);
            if size > REWRITE_THRESHOLD_BYTES {
                let started = std::time::Instant::now();
                match state.mailbox.store_ref().rewrite_aof() {
                    Ok(stats) => {
                        tracing::info!(
                            before_bytes = size,
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
