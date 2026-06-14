//! Periodic maildir reconcile (S2.2): the "never lose a message" backstop
//! for the notification-driven core.
//!
//! The mpsc consumer (S1.4) indexes delivered mail promptly in the common
//! case. This sweep only has to catch the rare dropped notification — a
//! consumer that was down, a channel saturated past the synchronous
//! fallback, or (later) a cross-process notify lost in flight. A delivered
//! maildir file with no `messages` row is picked up here and indexed.
//! Idempotent against the live path via the (mailbox_id, maildir_id)
//! uniqueness from S1.1: re-scanning an already-indexed file never
//! duplicates it (the reconcile only repairs files that are missing).

use std::sync::Arc;
use std::time::Duration;

use mailrs_mailbox::PgMailboxStore;

/// Reconcile sweep interval. The notification path handles the common case;
/// this sweep only catches the rare dropped notification, so hourly is
/// plenty.
const RECONCILE_INTERVAL: Duration = Duration::from_secs(3600);

/// Spawn the periodic reconcile task: one sweep at startup, then every
/// [`RECONCILE_INTERVAL`]. Cheap when there is nothing to repair — a clean
/// store scans the maildir tree and finds every file already indexed.
pub(crate) fn spawn_periodic_reconcile(store: Arc<PgMailboxStore>, maildir_root: String) {
    tokio::spawn(async move {
        // interval's first tick fires immediately → a sweep at startup.
        let mut ticker = tokio::time::interval(RECONCILE_INTERVAL);
        loop {
            ticker.tick().await;
            match store.reconcile_maildir(&maildir_root, false).await {
                Ok(report) if report.repaired > 0 => {
                    tracing::info!(
                        event = "reconcile_sweep",
                        scanned = report.scanned,
                        missing = report.missing,
                        repaired = report.repaired,
                        "periodic reconcile repaired orphaned maildir files"
                    );
                }
                Ok(report) => {
                    tracing::debug!(
                        event = "reconcile_sweep",
                        scanned = report.scanned,
                        "periodic reconcile: nothing to repair"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        event = "reconcile_sweep_failed",
                        error = %e,
                        "periodic reconcile failed"
                    );
                }
            }
        }
    });
}
