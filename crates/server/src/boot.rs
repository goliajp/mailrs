//! Booting the monolith-shaped process, and choosing which roles it plays.
//!
//! Split out of `lib.rs` on 2026-08-13, which was at 481 of the 500 allowed
//! and had no room for the role gates below.
//!
//! ## Roles
//!
//! This crate has always started every role at once — SMTP, the web tier,
//! IMAP, POP3, ManageSieve, outbound, the RBL monitor, the core-api contract.
//! That is what made it "the monolith", and it is the only thing that did:
//! the roles themselves are ordinary subsystems, and the SQL-side code for
//! each one is the peer of fastcore's.
//!
//! So `Roles` is what separates the two entry points. `run()` turns
//! everything on and is unchanged. `run_pg_core()` turns on the set that
//! makes this process **fastcore's peer** — the core-api contract, the spool
//! drain that indexes arrivals, and the protocols this side owns — and leaves
//! off the four that another process owns in that topology.
//!
//! One boot sequence rather than two, deliberately. A second copy of this
//! ordering would drift from the first, and the drift would show up as
//! "works on one core, not the other". A role gate cannot
//! drift from itself.

use std::sync::Arc;

use crate::*;

mod background;
mod context;
mod startup;

/// Which roles a process plays.
///
/// Only the four that differ between the two entry points are fields. Every
/// other subsystem is wanted by both, and a flag for it would be a knob
/// nobody turns.
#[derive(Clone, Copy)]
pub(crate) struct Roles {
    /// SMTP listeners on 25/587/465. `mailrs-receiver` owns these in the
    /// split topology, and it is the *only* SMTP entry there — two processes
    /// binding 25 is one bind error and one silent winner.
    pub(crate) smtp: bool,
    /// The web tier. `mailrs-webapi` owns it, and it is the public entry.
    pub(crate) web: bool,
    /// Outbound delivery. `mailrs-fastcore-sender` owns it; two drainers on
    /// one queue is how a message goes out twice.
    pub(crate) outbound: bool,
    /// The RBL reputation monitor — a periodic probe of our own IP against
    /// public blocklists. One process per host should run it, and the process
    /// that owns SMTP is the one that cares.
    pub(crate) rbl_monitor: bool,
    /// Drain the spool the receiver writes to, indexing each arrival into this
    /// process's backend. On in both role sets: the SMTP side only ever
    /// writes the spool, so a process that takes mail without draining would
    /// show an inbox that stops growing and report nothing wrong.
    pub(crate) spool_drain: bool,
}

impl Roles {
    /// Every role — the historical shape of this binary.
    pub(crate) const fn all() -> Self {
        Self {
            smtp: true,
            web: true,
            outbound: true,
            rbl_monitor: true,
            spool_drain: true,
        }
    }

    /// fastcore's peer: the core-api contract, the spool drain behind it, and
    /// the protocols this side serves. Nothing another process owns.
    ///
    /// Gated on the feature that compiles the contract: without `core-rpc`
    /// there is no `run_pg_core` to call this, and a role set for a process
    /// that cannot exist is dead code the default build is right to reject.
    #[cfg(feature = "core-rpc")]
    pub(crate) const fn peer() -> Self {
        Self {
            smtp: false,
            web: false,
            outbound: false,
            rbl_monitor: false,
            spool_drain: true,
        }
    }
}

/// Boot every store and the roles `roles` selects, then block until a
/// shutdown signal arrives.
/// then blocks until a shutdown signal. Lives in the library so both the
/// `mailrs-server` binary and integration tests share one crate root.
pub(crate) async fn run_with_roles(roles: Roles) {
    startup::install_tracing_and_crypto();

    let metrics_handle = metrics::install_prometheus_recorder();

    let cfg = ServerConfig::from_env();

    startup::log_startup(&cfg);

    let bootstrap::connections::Connections {
        event_bus,
        health_state,
        kevy_embedded_store,
        kevy_net_client,
        outbound_queue,
        pg_pool,
        rate_limiter,
        shutdown_rx,
        shutdown_tx,
        tls_state,
        user_store,
    } = bootstrap::connections::connect(&cfg).await;

    let bootstrap::shield::Shield {
        auth_guard,
        greylist_config,
        greylist_db,
        resolver,
    } = bootstrap::shield::build(&cfg, &pg_pool, &kevy_embedded_store, &kevy_net_client).await;

    let bootstrap::stores::Stores {
        alias_store,
        dmarc_report_store,
        domain_store,
        mailbox_store,
    } = bootstrap::stores::build(&cfg, &pg_pool, &kevy_embedded_store, &health_state).await;

    // shared LLM provider — used by background analyzer, web semantic
    // search, and inbound spam classification. Wrap once, clone everywhere.
    let bootstrap::services::Services {
        greylist_local_handle,
        ldap_config,
        llm_provider,
        system_config_store,
    } = bootstrap::services::build(
        &cfg,
        &pg_pool,
        &kevy_embedded_store,
        &mailbox_store,
        &event_bus,
        shutdown_rx.clone(),
    )
    .await;
    let web_state = Arc::new(build_web_state(WebStateInputs {
        cfg: &cfg,
        event_bus: event_bus.clone(),
        auth_guard: auth_guard.clone(),
        health_state: health_state.clone(),
        pg_pool: &pg_pool,
        kevy_embed: &kevy_embedded_store,
        outbound_queue: &outbound_queue,
        mailbox_store: &mailbox_store,
        domain_store: &domain_store,
        alias_store: &alias_store,
        llm_provider: &llm_provider,
        resolver: &resolver,
        ldap_config: &ldap_config,
        system_config_store: system_config_store.clone(),
        metrics_handle: metrics_handle.clone(),
        greylist_local: greylist_local_handle.clone(),
    }));

    background::spawn_ics_feed_worker(&pg_pool);

    let users = Arc::new(user_store);

    let greylist_whitelist = background::start_greylist_whitelist_sync(&cfg);

    let inbound_pipeline = build_inbound_pipeline_with_shadows(
        &greylist_db,
        &greylist_config,
        &greylist_whitelist,
        &greylist_local_handle,
        &resolver,
        &dmarc_report_store,
        &cfg,
        &llm_provider,
        &kevy_embedded_store,
    );

    let ctx = context::build_connection_context(context::ContextInputs {
        cfg: &cfg,
        tls_state: &tls_state,
        users: &users,
        event_bus: &event_bus,
        web_state: &web_state,
        rate_limiter,
        resolver,
        auth_guard: &auth_guard,
        domain_store: &domain_store,
        ldap_config: &ldap_config,
        inbound_pipeline,
    });

    background::start_spool_drain(roles, &ctx, &cfg, &domain_store, &mailbox_store);

    if roles.smtp {
        spawn_smtp_listeners(&ctx, &cfg, tls_state.is_some(), shutdown_rx.clone()).await;
    }

    if roles.web {
        spawn_web_server(web_state, &cfg, &domain_store, shutdown_rx.clone()).await;
    }

    spawn_mail_protocols(MailProtocols {
        mailbox_store: &mailbox_store,
        users: &users,
        domain_store: &domain_store,
        event_bus: &event_bus,
        ldap_config: &ldap_config,
        tls_state: &tls_state,
        cfg: &cfg,
    });

    if roles.outbound {
        spawn_outbound_delivery(
            outbound_queue.as_ref(),
            ctx.resolver.as_ref(),
            kevy_embedded_store.as_ref(),
            &cfg,
            event_bus.clone(),
            shutdown_rx.clone(),
        );
    }

    spawn_webhook_subsystem(
        &pg_pool,
        &event_bus,
        &system_config_store,
        shutdown_rx.clone(),
    );

    spawn_dmarc_aggregate_task(
        &dmarc_report_store,
        &ctx.resolver,
        &cfg,
        outbound_queue.clone(),
        shutdown_rx.clone(),
    );

    if roles.rbl_monitor {
        spawn_rbl_monitor(&ctx.resolver, &cfg.hostname, &kevy_embedded_store);
    }

    background::spawn_snooze_wake(&mailbox_store);

    #[cfg(feature = "core-rpc")]
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
