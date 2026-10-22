//! Phase 1 of `run()`: the connections every later phase needs — the
//! Postgres pool, the embedded and network kevy handles, TLS, the user
//! store, and the event bus.
//!
//! Lifted out of `lib.rs` verbatim on 2026-08-02.

use std::sync::Arc;

use super::*;
use crate::*;

pub(crate) struct Connections {
    pub(crate) event_bus: EventBus,
    pub(crate) kevy_embedded_store: Option<kevy_store::KevyStore>,
    pub(crate) pg_pool: Option<pg::BackendPool>,
    pub(crate) shutdown_rx: tokio::sync::watch::Receiver<bool>,
    pub(crate) shutdown_tx: tokio::sync::watch::Sender<bool>,
    pub(crate) tls_state: Option<crate::tls::TlsState>,
    pub(crate) user_store: UserStore,
}

pub(crate) async fn connect(cfg: &config::ServerConfig) -> Connections {
    // PG + Kevy connections (optional, graceful degradation)
    #[cfg(feature = "spg")]
    if cfg.spg_force_unlock
        && let Some(url) = &cfg.pg_url
    {
        pg::force_unlock(url);
    }
    let pg_pool = match &cfg.pg_url {
        // 5 min covers observed WAL-replay boot races with headroom;
        // past that, concede to degraded mode as before
        Some(url) => pg::connect_pool_with_retry(url, std::time::Duration::from_secs(300)).await,
        None => None,
    };
    // a core on the wrong schema answers every request wrongly; it does not
    // start instead
    if let Some(pool) = &pg_pool
        && let Err(e) = crate::pg_schema::ensure(pool).await
    {
        tracing::error!(error = %e, "sql schema");
        eprintln!("mailrs: sql schema: {e}");
        std::process::exit(1);
    }

    // kevy embedded store — in-process Arc<Store>, persistent if
    // cfg.kevy_data_dir is set. Health check exercises this path
    // (see health::spawn_health_checker). Phase C: only the in-process
    // store remains — every stone (shield greylist / intelligence spam
    // cache / outbound-queue notifier) now takes the same Store handle.
    let kevy_embedded_store: Option<kevy_store::KevyStore> =
        match kevy_store::open_store(cfg.kevy_data_dir.as_deref()) {
            Ok(store) => {
                tracing::info!(
                    persist_dir = ?cfg.kevy_data_dir,
                    "kevy embedded store opened"
                );
                Some(store)
            }
            Err(e) => {
                tracing::warn!(error = %e, "kevy embedded store open failed");
                None
            }
        };

    // The network kevy-server this process shares with the rest of the
    // fleet carries mail events across processes.
    let kevy_net_client: Option<Arc<kevy_net::KevyNetClient>> = cfg.kevy_url.as_ref().map(|url| {
        tracing::info!(
            kevy_url = %url,
            "mail events cross processes via network kevy-server"
        );
        Arc::new(kevy_net::KevyNetClient::new(url.clone()))
    });

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let tls_state = init_tls_state(cfg, shutdown_rx.clone()).await;

    let user_store = match &cfg.users_file {
        Some(path) => UserStore::load(path).expect("failed to load users file"),
        None => UserStore::empty(),
    };

    // In the receiver-split topology, mail events also cross processes
    // via a shared kevy-server: this process publishes its own and a
    // background bridge re-injects others' into the local bus (skipping
    // its own origin). Supplements — never replaces — the in-process
    // broadcast.
    let event_bus = match kevy_net_client.as_ref() {
        Some(client) => {
            let origin = kevy_notify::process_origin();
            let channel = kevy_notify::NOTIFY_CHANNEL.to_vec();
            let publisher = Arc::new(kevy_notify::KevyEventPublisher::new(
                client.clone(),
                channel.clone(),
                origin.clone(),
            ));
            let bus = EventBus::new(1024).with_publisher(publisher);
            kevy_notify::spawn_kevy_notify_bridge(
                client.url().to_string(),
                channel,
                origin,
                bus.clone(),
            );
            bus
        }
        None => EventBus::new(1024),
    };

    Connections {
        event_bus,
        kevy_embedded_store,
        pg_pool,
        shutdown_rx,
        shutdown_tx,
        tls_state,
        user_store,
    }
}
