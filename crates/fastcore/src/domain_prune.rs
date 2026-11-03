//! Letting go of first-seen domains seen once, long ago
//! (`core_sidestate::families::domain_first::prune`).
//!
//! Spam with random link domains adds a key per domain and nothing else
//! ever removes one. The sweep reads the keys and deletes only the ones
//! that qualify, so a pass with nothing to do writes nothing; when a pass
//! finds nothing, the next waits twice as long, up to a day, and a pass
//! that deletes something goes back to hourly.

use std::sync::Arc;
use std::time::Duration;

use crate::FastcoreState;

const BASE: Duration = Duration::from_secs(3600);
const MAX: Duration = Duration::from_secs(24 * 3600);

/// The next wait after a pass that deleted `pruned` keys.
fn next_wait(wait: Duration, pruned: u64) -> Duration {
    match pruned {
        0 => (wait * 2).min(MAX),
        _ => BASE,
    }
}

pub(crate) fn spawn(state: Arc<FastcoreState>) {
    tokio::spawn(async move {
        let mut wait = BASE;
        loop {
            tokio::time::sleep(wait).await;
            let st = state.clone();
            let pruned = tokio::task::spawn_blocking(move || {
                let mut conn = st.net_conn()?;
                let now = chrono::Utc::now().timestamp();
                mailrs_core_sidestate::families::domain_first::prune(&mut conn, now)
                    .map_err(|e| tracing::warn!(error = %e, "first-seen domain prune failed"))
                    .ok()
            })
            .await
            .ok()
            .flatten()
            .unwrap_or(0);
            if pruned > 0 {
                tracing::info!(pruned, "first-seen domains let go");
            }
            wait = next_wait(wait, pruned);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_idle_sweep_backs_off_to_a_day_and_work_resets_it() {
        let mut w = BASE;
        for _ in 0..10 {
            w = next_wait(w, 0);
        }
        assert_eq!(w, MAX);
        assert_eq!(next_wait(w, 3), BASE);
    }
}
