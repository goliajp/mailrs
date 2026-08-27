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
//! "works on one core, not the other" — the failure
//! `feedback-two-impls-need-a-contract-test` is about. A role gate cannot
//! drift from itself.

use std::sync::Arc;

use crate::*;

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
    /// process's backend. `all()` leaves this to `MAILRS_RECEIVER_SPLIT`, which
    /// is how the monolith has always opted in; the peer sets it outright,
    /// because a peer that does not drain shows an inbox that stops growing
    /// and reports nothing wrong.
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
            spool_drain: false,
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
        org_names: cfg.org_names.clone(),
        org_name_allowed_domains: cfg.org_name_allowed_domains.clone(),
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
    if roles.spool_drain
        || std::env::var("MAILRS_RECEIVER_SPLIT")
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
        tracing::info!("core spool consumer started — arrivals index into this backend");
    }

    if roles.smtp {
        spawn_smtp_listeners(&ctx, &cfg, tls_state.is_some(), shutdown_rx.clone()).await;
    }

    if roles.web {
        spawn_web_server(web_state, &cfg, &domain_store, shutdown_rx.clone()).await;
    }

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

    // Bring back the threads whose time has come.
    //
    // A snooze files the thread away — `archived` with the time beside it, the
    // same shape the kevy lane uses — so something has to un-archive it. A
    // predicate on the read cannot: the row is archived, and "archived because
    // asleep, and no longer asleep" is not a state the list can tell from
    // "archived on purpose".
    //
    // A minute is the resolution, matching the kevy side. A thread asked back
    // "tomorrow morning" arriving at 08:00:37 is the same promise kept, and a
    // second-accurate wake would cost sixty times the ticks to say nothing sixty
    // times as often.
    //
    // Logged only when something happened. A line every minute saying zero is
    // the shape that turned the maildir sweep's own idle report into the noise
    // hiding it (`rules/periodic-work-must-converge.md`).
    if let Some(mb) = mailbox_store.clone() {
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
            tick.tick().await; // the first tick is immediate; skip it
            loop {
                tick.tick().await;
                match mb.wake_snoozed().await {
                    Ok(0) => {}
                    Ok(n) => tracing::info!(woken = n, "snoozed threads returned"),
                    Err(e) => tracing::warn!(error = %e, "snooze wake failed"),
                }
            }
        });
        tracing::info!(event = "subsystem_started", subsystem = "snooze_wake");
    }

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
        // Detached on purpose. The monolith has no lock to release that this
        // task holds — its own shutdown path covers that — so the handle is
        // dropped rather than awaited. `drop` and not `let _ =`: a JoinHandle
        // is a future, and `let _ =` on one reads as "ignore the result" while
        // actually detaching it (clippy::let_underscore_future).
        drop(core_rpc::spawn_core_rpc(
            core_rpc_state,
            shutdown_rx.clone(),
        ));
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
