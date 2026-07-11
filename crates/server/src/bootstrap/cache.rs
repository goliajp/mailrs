#![allow(unused_imports)]
//! Subscribe to `SmtpEvent::NewMessage` and bust Kevy caches.

use std::sync::Arc;

use crate::config;
use crate::domain_store;
use crate::event_bus::{EventBus, SmtpEvent};
use crate::inbound::auth_guard::{AuthGuard, AuthGuardConfig};
use crate::web::WebState;
use crate::{
    acme, conversation_cache, dmarc_report, event_bus, health, listeners, oidc_jwt,
    outbound_tls_rpt, rbl_monitor, render_preview, search_index, smtp_session, system_config, tls,
    web, webhook,
};
use mailrs_mailbox::PgMailboxStore;

/// Subscribe to `SmtpEvent::NewMessage` and drop the Kevy cache
/// for the recipient's conversation list / categories + the
/// affected thread. Server + frontend caches stay coherent: WS
/// NewMessage triggers RQ invalidate on the client; this task does
/// the equivalent for the server cache so the next read goes back
/// to PG and picks up the new message.
///
/// No-op when Kevy isn't configured (no cache to bust).
pub(crate) fn spawn_cache_bust_task(
    kevy_store: &Option<crate::kevy_store::KevyStore>,
    event_bus: &EventBus,
) {
    let Some(store) = kevy_store else { return };
    let store = store.clone();
    let mut rx = event_bus.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(env) => {
                    if let event_bus::SmtpEvent::NewMessage {
                        user, thread_id, ..
                    } = &env.event
                    {
                        conversation_cache::bust_thread(&store, user, thread_id);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                _ => {}
            }
        }
    });
}
