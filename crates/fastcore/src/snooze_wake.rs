//! Bringing back the threads whose time has come.
//!
//! A snooze files the thread away and records the epoch second it is
//! due back; this asks the index for anything due and clears both
//! fields. Every ordinary membership row stores `0`, so the query is
//! `[1, now]` over a range that is empty until something is actually
//! waiting — the idle tick performs no writes at all, which is what
//! `periodic-work-must-converge` asks of anything on a timer.
//!
//! A minute is the resolution. A thread asked back "tomorrow morning"
//! arriving at 08:00:37 is the same promise kept; a second-accurate
//! wake would cost sixty times the ticks to say nothing sixty times
//! as often.

use std::sync::Arc;
use std::time::Duration;

use tokio::time::sleep;

use crate::FastcoreState;

const TICK: Duration = Duration::from_secs(60);

pub fn spawn(state: Arc<FastcoreState>) {
    tokio::spawn(async move {
        loop {
            sleep(TICK).await;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            match state.mailbox.wake_snoozed(now) {
                // Logged only when something happened. A line every
                // minute saying "0" is the shape that turned the
                // maildir sweep's own idle report into the noise
                // hiding it.
                Ok(0) => {}
                Ok(n) => tracing::info!(woken = n, "snoozed threads returned"),
                Err(e) => tracing::warn!(error = %e, "snooze wake failed"),
            }
        }
    });
}
