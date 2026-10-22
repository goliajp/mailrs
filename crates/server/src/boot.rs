//! Booting the SQL core: every store, then the subsystems it serves, then
//! block until a shutdown signal.
//!
//! What runs here is fastcore's peer set: the core-api contract, the spool
//! drain that indexes arrivals, IMAP / POP3 / ManageSieve, and the periodic
//! work both cores do. SMTP, the web tier and outbound delivery belong to
//! `mailrs-receiver`, `mailrs-webapi` and `mailrs-fastcore-sender`, which
//! serve either core.

use std::sync::Arc;

use crate::*;

mod background;
mod startup;

pub(crate) async fn run() {
    startup::install_tracing_and_crypto();

    let cfg = ServerConfig::from_env();

    startup::log_startup(&cfg);

    let bootstrap::connections::Connections {
        event_bus,
        kevy_embedded_store,
        pg_pool,
        shutdown_rx,
        shutdown_tx,
        tls_state,
        user_store,
    } = bootstrap::connections::connect(&cfg).await;

    let bootstrap::stores::Stores {
        domain_store,
        mailbox_store,
    } = bootstrap::stores::build(&cfg, &pg_pool, &kevy_embedded_store).await;

    let ldap_config = bootstrap::services::build(&cfg, &pg_pool, &mailbox_store, &event_bus);

    background::spawn_spool_drain(&event_bus, &cfg, &domain_store, &mailbox_store);

    spawn_mail_protocols(MailProtocols {
        mailbox_store: &mailbox_store,
        users: &Arc::new(user_store),
        domain_store: &domain_store,
        event_bus: &event_bus,
        ldap_config: &ldap_config,
        tls_state: &tls_state,
        cfg: &cfg,
    });

    spawn_global_webhook(&event_bus, &cfg, shutdown_rx.clone());

    background::spawn_snooze_wake(&mailbox_store);

    background::spawn_core_rpc_server(
        &cfg,
        &mailbox_store,
        &domain_store,
        &pg_pool,
        &event_bus,
        &shutdown_rx,
    );

    background::wait_for_shutdown_signal().await;
    tracing::info!("shutting down");
    let _ = shutdown_tx.send(true);
}
