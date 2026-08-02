mod account_store;
mod acme;
mod ai_analyzer;
mod api_key_store;
mod config;
mod conn_metrics;
mod metrics;

mod content_worker;
mod conversation_cache;

mod calendar;
mod dmarc_report;
mod domain_store;
mod event_bus;
mod health;
pub(crate) mod permission;

/// re-export shim: `LdapConfig` moved to the shared `mailrs-core` crate
/// (S5.2h). Kept as `crate::ldap_auth` so the smtp / imap / pop3 /
/// managesieve / web / config call sites stay unchanged.
mod ldap_auth {
    pub use mailrs_core::ldap_auth::*;
}

mod imap_session;
pub mod inbound;
mod inline_image;
/// re-export shim: the generic TCP listener template moved to
/// mailrs-receiver (P6-S5) so the receiver binary can bind its SMTP
/// listeners. server's imap/pop3/web/managesieve listeners use it via this.
mod listeners {
    pub use mailrs_receiver::listeners::*;
}
mod managesieve_session;
mod message_store;
mod message_util;
mod outbound_tls_rpt;
mod pg;
mod pop3_session;
mod quota_store;
mod rbl_monitor;
mod reconcile_task;
mod reputation;

mod bootstrap;
mod greylist_backfill;
// greylist_local keeps the spg-bound PG loaders + re-exports the pure
// snapshot/matching half from mailrs-receiver (S5.3).
mod greylist_local;
/// re-export shim: the remote-whitelist sync (spg-free) moved to
/// mailrs-receiver (S5.3).
mod greylist_sync {
    pub use mailrs_receiver::greylist_sync::*;
}
/// re-export shim: the network kevy client moved to mailrs-receiver (P6-S5)
/// so the receiver binary can construct the network anti backends.
pub mod kevy_net {
    pub use mailrs_receiver::kevy_net::*;
}
/// re-export shim: cross-process notify (publisher + subscriber bridge)
/// moved to mailrs-receiver (P6-S5) — the receiver publishes SpoolDelivered,
/// the core spawns the subscriber bridge. Both via this shim.
pub mod kevy_notify {
    pub use mailrs_receiver::kevy_notify::*;
}
#[cfg(feature = "core-rpc")]
mod core_rpc;
mod kevy_store;
mod mcp;
mod oidc_jwt;
mod oidc_store;
mod smtp_session;
pub(crate) mod system_config;
mod tls;
mod totp;
/// re-export shim: `UserStore` + credential helpers moved to the shared
/// `mailrs-core` crate (S5.2f). Kept as `crate::users` so the web / imap /
/// pop3 / mcp / smtp call sites stay unchanged.
mod users {
    pub use mailrs_core::users::*;
}
mod web;

use bootstrap::*;

mod webhook;

use std::sync::Arc;

use hickory_resolver::TokioResolver;

use crate::config::ServerConfig;
use crate::inbound::rate_limit::{InMemoryRateLimitStore, RateLimitStore, TokenBucketConfig};
use crate::smtp_session::ConnectionContext;
use crate::users::UserStore;
use mailrs_mailbox::PgMailboxStore;
use mailrs_shield::greylist::{GreylistConfig, GreylistDb};

// Re-export only the event types integration tests need to observe the
// receiving pipeline. The driver itself lives in `test_support`, which
// builds a real ConnectionContext internally — so none of the heavy
// server types (ConnectionContext, WebState, …) have to become public
// API and trip `private_interfaces` once this crate compiles as a lib.
pub use event_bus::{BroadcastEvent, EventBus, SmtpEvent};

#[doc(hidden)]
pub mod test_support;

/// Server entry point. Boots config, all stores, and every listener,
/// then blocks until a shutdown signal. Lives in the library so both the
/// `mailrs-server` binary and integration tests share one crate root.
pub async fn run() {
    // initialize structured logging via tracing-subscriber
    // respect RUST_LOG env var; default to info level
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install rustls crypto provider");

    let metrics_handle = metrics::install_prometheus_recorder();

    let cfg = ServerConfig::from_env();

    for warning in cfg.validate() {
        tracing::warn!(warning, "config warning");
    }

    let domains_str = if cfg.local_domains.is_empty() {
        "(none)".into()
    } else {
        cfg.local_domains.join(", ")
    };
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        hostname = cfg.hostname.as_str(),
        maildir = cfg.maildir_root.as_str(),
        domains = domains_str.as_str(),
        tls = ?cfg.tls_mode(),
        antispam = cfg.antispam_enabled,
        dkim = cfg.dkim_selector.as_deref().unwrap_or("(disabled)"),
        "mailrs starting"
    );

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

    // MRS-10: spawn external ICS feed worker. Cheap when no feeds exist —
    // the DUE query returns empty and the loop sleeps.
    if let Some(ref pool) = pg_pool {
        calendar::feed_worker::spawn_feed_worker(pool.clone());
        tracing::info!(event = "subsystem_started", subsystem = "external_ics_feed");
    }

    let users = Arc::new(user_store);

    // Greylist whitelist sync: fires once immediately, then every
    // cfg.greylist_sync_interval_secs. Empty URL = disabled, handle stays
    // empty (no hits, all senders go through the triplet check). The
    // handle is cloned into the GreylistStage so the sync task and the
    // stage share a single snapshot via tokio::sync::RwLock.
    let greylist_whitelist = greylist_sync::empty();
    if let Some(ref url) = cfg.greylist_whitelist_url {
        let handle = greylist_whitelist.clone();
        let url = url.clone();
        let interval = cfg.greylist_sync_interval_secs;
        tracing::info!(
            event = "subsystem_started",
            subsystem = "greylist_sync",
            url = %url,
            interval_secs = interval,
        );
        // monolith (staging dogfood lane): no disk cache — its whitelist
        // behaviour predates the receiver-split hardening and stays as-was
        greylist_sync::spawn_sync_task(handle, url, interval, None);
    } else {
        tracing::info!(
            event = "subsystem_skipped",
            subsystem = "greylist_sync",
            reason = "MAILRS_GREYLIST_WHITELIST_URL not set"
        );
    }

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

    // single post-delivery consumer (S1.4 + P5): DATA handlers hand
    // delivered messages here so maildir write stays on the hot path. The
    // core-side deps (mailbox store, event bus, calendar pool, resolver)
    // live with the consumer, not the receiver — only a plain
    // `DeliveredMessage` crosses the channel. `None` deps = degraded mode
    // (no mailbox store): the consumer drains and drops, reconcile indexes.
    let process_deps = mailbox_store.clone().map(|mb| {
        Arc::new(crate::smtp_session::ProcessDeps {
            mailbox_store: mb,
            event_bus: event_bus.clone(),
            outbound_queue: outbound_queue.clone(),
            resolver: resolver.clone(),
            maildir_root: cfg.maildir_root.clone(),
            kevy_url: std::env::var("MAILRS_KEVY_URL")
                .ok()
                .filter(|s| !s.is_empty()),
        })
    });
    let process_tx = crate::smtp_session::spawn_process_consumer(process_deps);

    let ctx = Arc::new(ConnectionContext {
        hostname: cfg.hostname.clone(),
        maildir_root: cfg.maildir_root.clone(),
        tls_state: tls_state.clone(),
        users: users.clone(),
        event_bus: event_bus.clone(),
        metrics: web_state.clone() as Arc<dyn mailrs_receiver::ConnectionMetrics>,
        rate_limiter,
        local_domains: cfg.local_domains.clone(),
        outbound_enqueue: outbound_queue.clone().map(|p| {
            Arc::new(mailrs_outbound_queue::PgQueueStore::new(p))
                as Arc<dyn mailrs_outbound_queue::QueueStore>
        }),
        resolver,
        dnsbl_zones: cfg.dnsbl_zones.clone(),
        dnsbl_enabled: cfg.dnsbl_enabled,
        antispam_enabled: cfg.antispam_enabled,
        quota_store: mailbox_store.clone().map(|m| {
            Arc::new(crate::quota_store::MailboxQuotaStore(m))
                as Arc<dyn mailrs_receiver::QuotaStore>
        }),
        smuggle_protection: cfg.smuggle_protection,
        auth_guard: auth_guard.clone(),
        account_store: domain_store
            .clone()
            .map(|d| d as Arc<dyn mailrs_receiver::AccountStore>),
        queue_notifier: kevy_embedded_store.as_ref().map(|s| {
            Arc::new(mailrs_outbound_queue::KevyNotifier::new(s.as_ref().clone()))
                as Arc<dyn mailrs_outbound_queue::Notifier>
        }),
        srs_secret: cfg.srs_secret.clone(),
        ldap_config: ldap_config.clone(),
        inbound_pipeline,
        // v2.4.1 Phase 3 (RFC-B §3.3): monolith has no shared kevy
        // sidecar handle to hand out — spam whitelist / blacklist
        // lookups are a fastcore-topology feature. `None` here means
        // the pipeline receives empty sets, identical to pre-Phase-3
        // behavior.
        spam_lists_client: None,
        delivery_executor: mailrs_delivery_executor::DeliveryExecutor::spawn(),
        process_tx,
        // monolith: inline delivery via process_tx (the receiver binary sets
        // this to a maildir spool sink for the split topology).
        spool_sink: None,
    });

    // P6 split: when this core runs as the consumer half
    // (MAILRS_RECEIVER_SPLIT), also drain the spool the receiver process
    // writes to — consume spool files (SpoolDelivered notify + reconcile
    // sweep) and run the same resolve/sieve/deliver/relay path, handing each
    // local delivery to the existing post-delivery consumer over a cloned
    // process_tx. Opt-in; the monolith (flag unset) is unchanged.
    if std::env::var("MAILRS_RECEIVER_SPLIT")
        .map(|v| v != "0" && v.to_lowercase() != "false")
        .unwrap_or(false)
    {
        let spool_root = std::env::var("MAILRS_SPOOL_ROOT")
            .unwrap_or_else(|_| format!("{}/.spool", cfg.maildir_root));
        let spool_deps = Arc::new(smtp_session::SpoolConsumeDeps {
            spool_incoming_path: format!("{spool_root}/incoming"),
            spool_store: crate::message_store::default_store(),
            delivery_executor: mailrs_delivery_executor::DeliveryExecutor::spawn(),
            process_tx: ctx.process_tx.clone(),
            account_store: ctx.account_store.clone(),
            quota_store: ctx.quota_store.clone(),
            outbound_enqueue: ctx.outbound_enqueue.clone(),
            queue_notifier: ctx.queue_notifier.clone(),
            event_bus: ctx.event_bus.clone(),
            hostname: cfg.hostname.clone(),
            srs_secret: cfg.srs_secret.clone(),
            local_domains: cfg.local_domains.clone(),
            maildir_root: cfg.maildir_root.clone(),
            in_flight: Arc::new(dashmap::DashMap::new()),
        });
        smtp_session::spawn_spool_consumer(spool_deps, ctx.event_bus.clone(), 30);
        tracing::info!("MAILRS_RECEIVER_SPLIT set: core spool consumer started");
    }

    spawn_smtp_listeners(&ctx, &cfg, tls_state.is_some(), shutdown_rx.clone()).await;

    spawn_web_server(web_state, &cfg, &domain_store, shutdown_rx.clone()).await;

    spawn_imap_listeners(
        &mailbox_store,
        &users,
        &auth_guard,
        &domain_store,
        &event_bus,
        &ldap_config,
        &tls_state,
        &cfg,
        shutdown_rx.clone(),
    )
    .await;

    spawn_pop3_listener(
        &mailbox_store,
        &users,
        &auth_guard,
        &domain_store,
        &ldap_config,
        &cfg,
        shutdown_rx.clone(),
    )
    .await;

    spawn_managesieve_listener(
        &users,
        &auth_guard,
        &domain_store,
        &ldap_config,
        &cfg,
        shutdown_rx.clone(),
    )
    .await;

    spawn_outbound_delivery(
        outbound_queue.as_ref(),
        ctx.resolver.as_ref(),
        kevy_embedded_store.as_ref(),
        &cfg,
        event_bus.clone(),
        shutdown_rx.clone(),
    );

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

    spawn_rbl_monitor(&ctx.resolver, &cfg.hostname, &kevy_embedded_store);

    // Phase 2 — optional core RPC server (only compiled with --features core-rpc).
    // Default build excludes this entirely; production artifact is byte-identical.
    #[cfg(feature = "core-rpc")]
    if let (Some(mb), Some(ds), Some(pool)) = (
        mailbox_store.as_ref(),
        domain_store.as_ref(),
        pg_pool.as_ref(),
    ) {
        let core_rpc_state = std::sync::Arc::new(core_rpc::CoreRpcState {
            mailbox: mb.clone(),
            domain: ds.clone(),
            pool: pool.clone(),
            maildir_root: cfg.maildir_root.clone(),
            net_url: std::env::var("MAILRS_KEVY_URL")
                .ok()
                .filter(|s| !s.is_empty()),
        });
        core_rpc::spawn_core_rpc(core_rpc_state, shutdown_rx.clone());
    }

    // keep main alive — exit on SIGINT (interactive ctrl+c) or
    // SIGTERM (docker stop / compose recreate). SIGTERM matters for
    // the embedded spg catalog lock: dying without running Drop
    // leaves /data/spg/*.lock behind, and since spg 7.27 a
    // replacement container sees a foreign-namespace lock as
    // undecidable and refuses to open the catalog (v1.7.150 deploy
    // came up degraded for exactly this reason).
    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler");
        tokio::select! {
            r = tokio::signal::ctrl_c() => r.expect("failed to listen for ctrl+c"),
            _ = sigterm.recv() => {}
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for ctrl+c");
    tracing::info!("shutting down");
    let _ = shutdown_tx.send(true);
}
