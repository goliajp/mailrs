//! The loop that runs the upload sweep, and its figure for /api/health.
//!
//! The rule and the pass are `mailrs_core_sidestate::families::uploads::sweep`;
//! this process runs it because one webapi serves either core.

use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use mailrs_core_sidestate::families::uploads::{
    self,
    sweep::{self, BASE_INTERVAL, Pass},
};

fn pass(dry_run: bool) -> std::io::Result<Pass> {
    crate::kevy_pool::with_conn(|c| sweep::sweep(c, &uploads::root(), SystemTime::now(), dry_run))
}

/// Sweep after [`BASE_INTERVAL`], then back off while there is nothing to
/// do (`sweep::next_interval`).
pub fn spawn() {
    tokio::spawn(async {
        let mut wait = BASE_INTERVAL;
        loop {
            tokio::time::sleep(wait).await;
            match tokio::task::spawn_blocking(|| pass(false)).await {
                Ok(Ok(p)) => {
                    if p.did_work() {
                        tracing::info!(
                            deleted = p.deleted,
                            kept_for_drafts = p.expired_referenced,
                            images_kept = p.images_kept,
                            "uploads: swept"
                        );
                    }
                    wait = sweep::next_interval(wait, &p);
                }
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "uploads: sweep could not read the drafts")
                }
                Err(e) => tracing::warn!(error = %e, "uploads: sweep task failed"),
            }
        }
    });
}

/// For /api/health: the upload store, with how many files a sweep would
/// delete now (0 right after one). Counted at most once every five minutes;
/// null when the drafts cannot be read.
pub fn health() -> serde_json::Value {
    static LAST: Mutex<Option<(Instant, serde_json::Value)>> = Mutex::new(None);
    let cached = LAST
        .lock()
        .ok()
        .and_then(|last| last.clone())
        .filter(|(at, _)| at.elapsed() < Duration::from_secs(300))
        .map(|(_, v)| v);
    let expired = match cached {
        Some(v) => v,
        None => {
            let v = crate::handlers::kevy_util::off_reactor(|| pass(true))
                .map_or(serde_json::Value::Null, |p| p.expired_unreferenced.into());
            if let Ok(mut last) = LAST.lock() {
                *last = Some((Instant::now(), v.clone()));
            }
            v
        }
    };
    crate::handlers::jmap::upload_health(expired)
}
