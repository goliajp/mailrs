//! This core's half of the spool drain both cores share.
//!
//! The receiver writes accepted mail to the spool; `mailrs_intake` decides
//! when to read it and where each recipient's copy goes, against this
//! store's directory ([`PgDirectory`]), and hands a copy that reached one
//! of our mailboxes back to [`PgCore`].

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use mailrs_intake::spool::SpoolDirs;

mod core;
mod directory;
mod ingest;
mod store;

pub(crate) use core::PgCore;
pub(crate) use directory::PgDirectory;

/// Drain `spool_root` into `core` forever: now, whenever a message is
/// spooled, and at least every `interval`.
pub(crate) fn spawn_drain(core: Arc<PgCore>, spool_root: &Path, interval: Duration) {
    let dirs = SpoolDirs::under(spool_root);
    tracing::info!(
        spool_new = %dirs.new.display(),
        spool_cur = %dirs.cur.display(),
        maildir_root = %core.dir.maildir_root,
        interval_secs = interval.as_secs(),
        "spool drain started"
    );
    let net_url = core.dir.net_url.clone();
    tokio::spawn(mailrs_intake::spool::run(interval, net_url, move || {
        let (core, dirs) = (core.clone(), dirs.clone());
        async move {
            // the drain blocks on the store, so it runs off the runtime workers
            tokio::task::spawn_blocking(move || mailrs_intake::spool::tick(&dirs, &*core))
                .await
                .expect("spool drain tick");
        }
    }));
}

#[cfg(all(test, feature = "spg"))]
pub(crate) mod tests;
