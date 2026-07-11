#![allow(unused_imports)]
//! Runtime-editable system config store init + DB-reload task.

use std::sync::Arc;

use crate::config;
use crate::domain_store;
use crate::event_bus::{EventBus, SmtpEvent};
use crate::inbound::auth_guard::{AuthGuard, AuthGuardConfig};
use crate::web::WebState;
use crate::{
    acme, conversation_cache, dmarc_report, event_bus, health, listeners, oidc_jwt,
    outbound_tls_rpt, rbl_monitor, search_index, smtp_session, system_config, tls, web, webhook,
};
use mailrs_mailbox::PgMailboxStore;

/// Initialize the runtime-editable system config store, hydrate
/// from PG if available, and spawn the background reload task
/// that picks up DB changes without a restart.
pub(crate) async fn init_system_config_store(
    cfg: &config::ServerConfig,
    pg_pool: &Option<crate::pg::BackendPool>,
    kevy_store: &Option<crate::kevy_store::KevyStore>,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> Arc<system_config::SystemConfigStore> {
    let env_defaults = system_config::RuntimeConfig::from_server_config(cfg);
    let store = Arc::new(system_config::SystemConfigStore::new(
        pg_pool.clone(),
        kevy_store.clone(),
        env_defaults,
    ));
    if pg_pool.is_some()
        && let Err(e) = store.load_from_db().await
    {
        tracing::warn!("failed to load system config from DB: {e}");
    }
    let store_bg = store.clone();
    tokio::spawn(async move {
        system_config::reload_task(store_bg, shutdown_rx).await;
    });
    store
}
