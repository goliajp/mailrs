//! Spool drain — fastcore side of the receiver/core split.
//!
//! The receiver accepts SMTP and writes `{spool_root}/incoming/{new,cur}/*`
//! files with an `X-Mailrs-Spool-Envelope` header prepended. The shared
//! drain (`mailrs_intake`) reads them, routes each recipient, runs the
//! report hooks and sieve, and writes the copy to
//! `{maildir_root}/<domain>/<local>/new/`; this core then threads and
//! indexes it at once (`recipient`, `ingest_delivered_file`). The periodic
//! maildir self-heal picks up any file that delivery wrote and did not index.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use mailrs_intake::spool::SpoolDirs;

use crate::FastcoreState;

mod check;
mod core;
pub(crate) use core::Fastcore;
mod deliver;
mod recipient;
pub mod relay;

pub(crate) use check::recipient_deliverable_route;

/// Spawn the drain loop. Env:
/// - `MAILRS_SPOOL_ROOT` — spool base, default `/data/.spool`
/// - `MAILRS_MAILDIR`    — maildir root, default `/data/maildir`
/// - `MAILRS_FASTCORE_SPOOL_INTERVAL_SECS` — poll interval, default 15;
///   a spooled message wakes it sooner
///
/// If the spool `incoming/` dir doesn't exist yet, the loop still runs
/// — receiver may not have written its first file. Missing-dir errors
/// downgrade to debug.
pub async fn spawn(state: Arc<FastcoreState>) {
    let maildir_root =
        std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".to_string());
    let spool_root = mailrs_core::spool::spool_root(&maildir_root);
    let interval_secs: u64 = std::env::var("MAILRS_FASTCORE_SPOOL_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(15);
    let dirs = SpoolDirs::under(&PathBuf::from(&spool_root));
    tracing::info!(
        spool_new = %dirs.new.display(),
        spool_cur = %dirs.cur.display(),
        maildir_root = %maildir_root,
        interval_secs,
        "fastcore spool drain started"
    );
    let core = core::Fastcore::new(&state, &maildir_root);
    mailrs_intake::spool::run(
        Duration::from_secs(interval_secs),
        state.net_url.clone(),
        || {
            mailrs_intake::spool::tick(&dirs, &core);
            std::future::ready(())
        },
    )
    .await;
}

/// Walk one spool dir once, deliver every decodable file to its
/// recipient maildir(s), and remove it. Returns delivered count and
/// every filename walked.
pub fn drain_once(
    dir: &Path,
    maildir_root: &str,
    state: &Arc<FastcoreState>,
) -> (usize, HashSet<String>) {
    mailrs_intake::drain_once(dir, &core::Fastcore::new(state, maildir_root))
}
