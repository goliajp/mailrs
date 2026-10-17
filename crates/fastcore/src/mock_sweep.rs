//! Hourly removal of expired mock mail.
//!
//! A sweep with nothing to remove only reads, and consecutive empty sweeps
//! back off to the idle interval.

use std::sync::Arc;
use std::time::Duration;

use mailrs_core_sidestate::families::mock;

use crate::FastcoreState;

const BUSY_INTERVAL: Duration = Duration::from_secs(3600);
const IDLE_INTERVAL: Duration = Duration::from_secs(6 * 3600);

pub fn spawn(state: Arc<FastcoreState>) {
    if mock::domain().is_empty() {
        return;
    }
    tokio::spawn(async move {
        let mut idle_rounds = 0u32;
        loop {
            let s = state.clone();
            let changed = tokio::task::spawn_blocking(move || sweep_once(&s))
                .await
                .unwrap_or(false);
            idle_rounds = if changed {
                0
            } else {
                idle_rounds.saturating_add(1)
            };
            let wait = crate::idle_backoff::idle_backoff(BUSY_INTERVAL, IDLE_INTERVAL, idle_rounds);
            tokio::time::sleep(wait).await;
        }
    });
}

fn sweep_once(state: &FastcoreState) -> bool {
    let Some(mut conn) = state.net_conn() else {
        return false;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    match mock::sweep(&mut conn, mock::root(), now) {
        Ok(report) => {
            if report.changed_anything() {
                tracing::info!(
                    expired = report.expired,
                    orphan_files = report.orphan_files,
                    bytes_corrected = report.bytes_corrected,
                    "mock sweep"
                );
            }
            report.changed_anything()
        }
        Err(e) => {
            tracing::warn!(error = %e, "mock sweep failed");
            false
        }
    }
}
