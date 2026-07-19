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

    // Optional shared network kevy-server for the anti subsystems
    // (greylist / rate / auth-guard). Set MAILRS_KEVY_URL to point this
    // process at a kevy-server it shares with the rest of the fleet (the
    // receiver-split topology); unset keeps every subsystem on the
    // in-process embedded store. The embedded store always opens anyway
    // for the message-state hot path.
    let kevy_net_client: Option<Arc<kevy_net::KevyNetClient>> = cfg.kevy_url.as_ref().map(|url| {
        tracing::info!(
            kevy_url = %url,
            "anti subsystems will share state via network kevy-server"
        );
        Arc::new(kevy_net::KevyNetClient::new(url.clone()))
    });

    let health_state = health::HealthState::new();
    if let (Some(pg), Some(embed)) = (&pg_pool, &kevy_embedded_store) {
        health::spawn_health_checker(pg.clone(), embed.clone(), health_state.clone());
        health_state.set_pg(true);
        health_state.set_kevy(true);
    }

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let tls_state = init_tls_state(&cfg, shutdown_rx.clone()).await;

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

    spawn_cache_bust_task(&kevy_embedded_store, &event_bus);

    let rate_limit_config = TokenBucketConfig {
        capacity: cfg.rate_limit_capacity,
        refill_rate: cfg.rate_limit_refill,
    };
    // shared kevy-server → distributed fixed-window counter; else the
    // in-process GCRA token bucket.
    let rate_limiter: Arc<dyn RateLimitStore> = match kevy_net_client.as_ref() {
        Some(client) => Arc::new(
            crate::inbound::kevy_backends::KevyServerRateLimitStore::new(
                client.clone(),
                rate_limit_config,
            ),
        ),
        None => Arc::new(InMemoryRateLimitStore::new(rate_limit_config)),
    };

    let outbound_queue = pg_pool.clone();

    // DNS resolver for DNSBL and other lookups. Bumped cache from
    // hickory's default 32 entries to 4096 — at modest steady-state
    // mail traffic, 32 entries holds maybe a minute of unique
    // lookups; SPF/DKIM/DMARC queries hammer the same sender domain
    // up to four ways and benefit hugely from staying in cache. The
    // working set is bounded by unique sender domains × policy
    // record types (~4) which for any realistic load fits in 4096.
    let resolver = TokioResolver::builder_tokio()
        .ok()
        .and_then(|mut b| {
            b.options_mut().cache_size = 4096;
            b.build().ok()
        })
        .map(Arc::new);

    // PTR record check
    if let Some(ref r) = resolver {
        mailrs_shield::ptr::check_ptr_record(r, &cfg.hostname).await;
    }

    // greylisting (in-process kevy only; kevy AOF is durable).
    //
    // Until v1.7.108 we wired GreylistDb.with_pg(pool) so every hot-path
    // check mirrored to the PG `greylist_triplets` table — a belt-and-
    // suspenders durability hedge from the pre-AOF era. kevy-embedded
    // 1.1.6 ships forward-compat AOF persistence, so the mirror is no
    // longer earning its cost (one PG INSERT per inbound check is
    // measurable at SMTP-peak load).
    //
    // We still want the historical reputation in PG — months of
    // legitimate-sender first_seen timestamps — so on startup we
    // backfill the table into kevy once (idempotent via a sentinel
    // key). After that the hot path is pure kevy and the PG table
    // is read-only / archival.
    let greylist_config = GreylistConfig {
        initial_delay_secs: cfg.greylist_delay_secs,
        ..Default::default()
    };

    // Backfill PG reputation into the embedded store only when greylist
    // actually reads that store. In network mode greylist reads the
    // shared kevy-server, so warming the embedded store would be wasted.
    if kevy_net_client.is_none()
        && let (Some(store), Some(pool)) = (kevy_embedded_store.as_ref(), pg_pool.as_ref())
    {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        greylist_backfill::backfill_from_pg_best_effort(
            pool,
            store.as_ref(),
            greylist_config.pass_ttl_secs,
            now_secs,
        )
        .await;
    }

    // greylist reads the shared network kevy-server when MAILRS_KEVY_URL
    // is set, else the in-process embedded store.
    let greylist_db = match kevy_net_client.as_ref() {
        Some(client) => Some(Arc::new(GreylistDb::with_backend(Arc::new(
            crate::inbound::kevy_backends::KevyServerGreylistBackend::new(client.clone()),
        )))),
        None => kevy_embedded_store
            .as_ref()
            .map(|store| Arc::new(GreylistDb::new(store.as_ref().clone()))),
    };

    // shared kevy-server → distributed lockout shared across the fleet;
    // else the in-process AuthGuard (with its periodic cleanup task).
    let auth_guard: Arc<dyn crate::inbound::auth_guard::AuthGuardStore> =
        match kevy_net_client.as_ref() {
            Some(client) => Arc::new(
                crate::inbound::kevy_backends::KevyServerAuthGuardStore::new(
                    client.clone(),
                    auth_guard_config(&cfg),
                ),
            ),
            None => init_auth_guard(&cfg),
        };

    // mailbox store for IMAP (PG-backed)
    let mailbox_store = pg_pool
        .as_ref()
        .map(|pool| Arc::new(PgMailboxStore::new(pool.clone())));

    // AliasStore backend selector — RFC 20260705 Step 3. Constructed
    // ahead of DomainStore so it can be attached to both the store's
    // internal alias path AND the WebState top-level field, keeping
    // both sides on the same Arc. Env `MAILRS_ALIAS_STORE_BACKEND`:
    // `network` + `MAILRS_KEVY_URL` = shared network kevy (v2 dual-mode
    // sync); anything else = None → legacy PG-backed DomainStore.aliases.
    let alias_store: Option<Arc<dyn mailrs_alias_store::AliasStore>> = match std::env::var(
        "MAILRS_ALIAS_STORE_BACKEND",
    )
    .as_deref()
    {
        Ok("network") => match std::env::var("MAILRS_KEVY_URL") {
            Ok(url) if !url.is_empty() => {
                tracing::info!(
                    url = %url,
                    "alias-store backend = network kevy (monolith, RFC 20260705 Step 3)"
                );
                Some(Arc::new(
                    mailrs_alias_store_net::NetworkKevyAliasStore::new(url),
                ))
            }
            _ => {
                tracing::warn!(
                    "MAILRS_ALIAS_STORE_BACKEND=network but MAILRS_KEVY_URL is unset — falling back to PG-backed DomainStore.aliases"
                );
                None
            }
        },
        _ => {
            tracing::info!("alias-store backend = PG (DomainStore.aliases, default)");
            None
        }
    };

    // domain store (PG + Kevy + process cache); attach the alias_store
    // seam so its `resolve_recipient` alias steps + CRUD go through the
    // trait when a network backend is configured.
    let domain_store = if pg_pool.is_some() {
        let mut ds = domain_store::DomainStore::new(
            pg_pool.clone(),
            kevy_embedded_store.clone(),
            health_state.clone(),
        );
        if let Some(ref store) = alias_store {
            ds = ds.with_alias_store(store.clone());
        }
        let ds = Arc::new(ds);
        ds.preload_accounts().await;
        tracing::info!("domain store ready (PG-backed)");
        Some(ds)
    } else {
        None
    };

    // OIDC provider: ensure signing key exists
    if let Some(ref pool) = pg_pool
        && let Err(e) = oidc_jwt::ensure_signing_key(pool).await
    {
        tracing::warn!(error = %e, "failed to ensure oidc signing key");
    }

    // DMARC report store (PG-backed)
    let dmarc_report_store = pg_pool
        .as_ref()
        .map(|pool| Arc::new(dmarc_report::DmarcReportStore::new(pool.clone())));

    // backfill threading data for existing messages
    if let Some(ref mb) = mailbox_store {
        let maildir = cfg.maildir_root.clone();
        let count = mb.backfill_threading(&maildir).await;
        if count > 0 {
            tracing::info!(event = "threading_backfill_complete", count);
        }
    }

    // periodic maildir reconcile (S2.2): the "never lose a message"
    // backstop for the notification-driven post-delivery path.
    if let Some(ref mb) = mailbox_store {
        reconcile_task::spawn_periodic_reconcile(mb.clone(), cfg.maildir_root.clone());
    }

    // shared LLM provider — used by background analyzer, web semantic
    // search, and inbound spam classification. Wrap once, clone everywhere.
    let llm_provider: Option<Arc<dyn mailrs_intelligence::provider::LlmProvider>> =
        if cfg.ai_analysis_enabled {
            let model_id = format!(
                "qwen3.5-9b/{}",
                mailrs_intelligence::analyze::PROMPT_VERSION
            );
            Some(Arc::new(
                mailrs_intelligence::OpenAiCompatibleProvider::new(
                    cfg.llm_url.clone(),
                    cfg.llm_api_key.clone(),
                    model_id,
                ),
            ))
        } else {
            None
        };

    // AI email analyzer (background) — uses self-hosted LLM
    if let (Some(provider), Some(mb)) = (llm_provider.as_ref(), mailbox_store.as_ref()) {
        ai_analyzer::spawn_analyzer(
            provider.clone(),
            mb.clone(),
            event_bus.clone(),
            cfg.maildir_root.clone(),
        );
    }

    // content extraction worker (OCR, PDF text)
    if let Some(ref pool) = pg_pool {
        content_worker::spawn_content_worker(pool.clone(), cfg.maildir_root.clone());
    }

    // LDAP authentication backend (optional)
    let ldap_config = cfg.ldap_config().map(Arc::new);
    if ldap_config.is_some() {
        tracing::info!("LDAP authentication enabled");
    }

    let system_config_store =
        init_system_config_store(&cfg, &pg_pool, &kevy_embedded_store, shutdown_rx.clone()).await;

    // Phase 2 local greylist lists: load synchronously before WebState +
    // pipeline are wired so the very first inbound mail honors operator
    // policy. PG unavailable at boot = empty handle, same degradation
    // posture as Phase 1 sync.
    let greylist_local_handle = greylist_local::empty();
    if let Some(ref pool) = pg_pool {
        let started = std::time::Instant::now();
        greylist_local::reload(&greylist_local_handle, pool).await;
        let snapshot = greylist_local_handle.read().await;
        tracing::info!(
            event = "subsystem_started",
            subsystem = "greylist_local",
            white = snapshot.white_count(),
            black = snapshot.black_count(),
            total = snapshot.total(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            reload_secs = cfg.greylist_local_reload_secs,
            "greylist_local: snapshot loaded",
        );
        drop(snapshot);
        greylist_local::spawn_reload_task(
            greylist_local_handle.clone(),
            pool.clone(),
            cfg.greylist_local_reload_secs,
        );
    } else {
        tracing::info!(
            event = "subsystem_skipped",
            subsystem = "greylist_local",
            reason = "no postgres pool",
        );
    }

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
