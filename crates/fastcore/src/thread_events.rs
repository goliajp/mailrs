//! Where this core queues the thread events its routes raise
//! (`mailrs_core_api::server::thread_events`): the network kevy's webhook
//! outbox, off the runtime.

use std::sync::Arc;

use mailrs_core_api::server::{ThreadEventHit, ThreadEventSink};

use crate::FastcoreState;

pub(crate) fn sink(state: Arc<FastcoreState>) -> ThreadEventSink {
    Arc::new(move |hit: ThreadEventHit| {
        let state = state.clone();
        tokio::task::spawn_blocking(move || {
            let Some(mut conn) = state.net_conn() else {
                return;
            };
            mailrs_core_sidestate::families::webhook_events::thread_changed(
                &mut conn,
                &hit.user,
                &hit.thread_id,
                hit.label_id.as_deref(),
                hit.change,
                chrono::Utc::now().timestamp(),
            );
        });
    })
}
