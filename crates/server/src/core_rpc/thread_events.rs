//! Where this core queues the thread events its routes raise
//! (`mailrs_core_api::server::thread_events`): the network kevy's webhook
//! outbox, off the runtime. The same as fastcore's, over this core's URL.
//!
//! Each one also tells the user's open web clients that their mailbox
//! changed. fastcore says so from its store's change feed, which sees every
//! writer; this core has no such feed, so a change made over IMAP here
//! reaches the web clients only when they next refetch.

use std::sync::Arc;

use mailrs_core_api::server::{ThreadEventHit, ThreadEventSink};

use super::CoreRpcState;

pub(super) fn sink(state: Arc<CoreRpcState>) -> ThreadEventSink {
    Arc::new(move |hit: ThreadEventHit| {
        let Some(url) = state.net_url.clone() else {
            return;
        };
        tokio::task::spawn_blocking(move || {
            let Ok(mut conn) = kevy_client::Connection::connect(&url) else {
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
            let _ =
                mailrs_core_sidestate::families::live_events::mailbox_changed(&mut conn, &hit.user);
        });
    })
}
