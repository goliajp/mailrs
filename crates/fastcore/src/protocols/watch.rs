//! IDLE's view of the embedded kevy change feed.
//!
//! The feed is durable, so a fastcore restart resumes from the last
//! offset rather than losing what changed meanwhile. It is polled every
//! 500 ms, well within RFC 2177's "frequently enough that clients see
//! mail within a few seconds".

use std::sync::Arc;
use std::time::Duration;

use mailrs_mail_protocols::backend::Watch;
use tokio::time::{Interval, MissedTickBehavior};

use crate::FastcoreState;

pub struct FeedWatch(Option<Feed>);

struct Feed {
    state: Arc<FastcoreState>,
    prefix: String,
    at: (u64, u64),
    tick: Interval,
}

impl FeedWatch {
    /// Changes under the user's keys.
    pub(crate) fn new(state: Arc<FastcoreState>, user: &str) -> Self {
        let at = state.mailbox.store_ref().changes_tail().unwrap_or((0, 0));
        let mut tick = tokio::time::interval(Duration::from_millis(500));
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        Self(Some(Feed {
            state,
            prefix: format!("mailrs:user:{user}:"),
            at,
            tick,
        }))
    }

    /// Nothing ever changes (a mock namespace).
    pub(crate) fn never() -> Self {
        Self(None)
    }
}

impl Watch for FeedWatch {
    async fn changed(&mut self) {
        let Some(feed) = self.0.as_mut() else {
            return std::future::pending().await;
        };
        loop {
            feed.tick.tick().await;
            let batch = feed.state.mailbox.store_ref().changes_since(
                feed.at.0,
                feed.at.1,
                100,
                &[feed.prefix.as_bytes()],
            );
            // Disabled | Resync: skipped; a re-select recovers
            let Ok(batch) = batch else { continue };
            feed.at = batch.next;
            if !batch.changes.is_empty() {
                return;
            }
        }
    }
}
