mod acme;
mod ai_analyzer;
mod api_key_store;
mod config;
mod metrics;

mod content_worker;
mod conversation_cache;

mod calendar;
mod dmarc_report;
mod domain_store;
mod event_bus;
mod health;
pub(crate) mod permission;

mod ldap_auth;

mod imap_session;
pub mod inbound;
mod inline_image;
mod listeners;
mod managesieve_session;
mod message_util;
mod outbound_tls_rpt;
mod pg;
mod pop3_session;
mod rbl_monitor;
mod render_preview;
mod reputation;
mod search_index;

mod bootstrap;
mod kevy_store;
mod mcp;
mod oidc_jwt;
mod oidc_store;
mod smtp_session;
pub(crate) mod system_config;
mod tls;
mod totp;
mod users;
mod web;

use bootstrap::*;

mod webhook;

use std::sync::Arc;

use hickory_resolver::TokioResolver;

use crate::config::ServerConfig;
use crate::event_bus::EventBus;
use crate::inbound::rate_limit::{RateLimiter, TokenBucketConfig};
use crate::smtp_session::ConnectionContext;
use crate::users::UserStore;
use mailrs_mailbox::PgMailboxStore;
use mailrs_shield::greylist::{GreylistConfig, GreylistDb};

#[tokio::main]
async fn main() {
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
    let pg_pool = match &cfg.pg_url {
        Some(url) => match pg::create_pool(url).await {
            Ok(pool) => {
                tracing::info!("postgres connected");
                Some(pool)
            }
            Err(e) => {
                tracing::warn!(error = %e, "postgres connection failed, running in degraded mode");
                None
            }
        },
        None => None,
    };

    let kevy_conn = match &cfg.kevy_url {
        Some(url) => match kevy_store::create_connection(url).await {
            Ok(conn) => {
                tracing::info!("kevy connected");
                Some(conn)
            }
            Err(e) => {
                tracing::warn!(error = %e, "kevy connection failed, running in degraded mode");
                None
            }
        },
        None => None,
    };

    // kevy embedded store — parallel path for cement code that wants the
    // in-process Arc<Store> (the migration target). Currently unused by
    // the network-path subsystems; future commits migrate them over and
    // eventually drop the network `kevy_conn` entirely.
    let _kevy_embedded_store: Option<kevy_store::KevyStore> =
        match kevy_store::open_store(cfg.kevy_data_dir.as_deref()) {
            Ok(store) => {
                tracing::info!(
                    persist_dir = ?cfg.kevy_data_dir,
                    "kevy embedded store opened (parallel to network kevy)"
                );
                Some(store)
            }
            Err(e) => {
                tracing::warn!(error = %e, "kevy embedded store open failed");
                None
            }
        };

    let health_state = health::HealthState::new();
    if let (Some(pg), Some(vk)) = (&pg_pool, &kevy_conn) {
        health::spawn_health_checker(pg.clone(), vk.clone(), health_state.clone());
        health_state.set_pg(true);
        health_state.set_kevy(true);
    }

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let tls_state = init_tls_state(&cfg, shutdown_rx.clone()).await;

    let user_store = match &cfg.users_file {
        Some(path) => UserStore::load(path).expect("failed to load users file"),
        None => UserStore::empty(),
    };

    let event_bus = EventBus::new(1024);

    spawn_cache_bust_task(&kevy_conn, &event_bus);

    let rate_limiter = Arc::new(RateLimiter::new(TokenBucketConfig {
        capacity: cfg.rate_limit_capacity,
        refill_rate: cfg.rate_limit_refill,
    }));

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

    // greylisting (Kevy primary + PG cold backup)
    let greylist_db = kevy_conn.as_ref().map(|vk| {
        let db = GreylistDb::new(vk.clone());
        let db = if let Some(ref pool) = pg_pool {
            db.with_pg(pool.clone())
        } else {
            db
        };
        Arc::new(db)
    });

    let greylist_config = GreylistConfig {
        initial_delay_secs: cfg.greylist_delay_secs,
        ..Default::default()
    };

    let auth_guard = init_auth_guard(&cfg);

    // mailbox store for IMAP (PG-backed)
    let mailbox_store = pg_pool
        .as_ref()
        .map(|pool| Arc::new(PgMailboxStore::new(pool.clone())));

    // domain store (PG + Kevy + process cache)
    let domain_store = if pg_pool.is_some() {
        let ds = Arc::new(domain_store::DomainStore::new(
            pg_pool.clone(),
            kevy_conn.clone(),
            health_state.clone(),
        ));
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

    let meili_client = cfg.meili_url.as_ref().map(|url| {
        let key = cfg.meili_key.clone().unwrap_or_default();
        Arc::new(search_index::MeiliClient::new(url.clone(), key))
    });

    let system_config_store =
        init_system_config_store(&cfg, &pg_pool, &kevy_conn, shutdown_rx.clone()).await;

    let web_state = Arc::new(build_web_state(WebStateInputs {
        cfg: &cfg,
        event_bus: event_bus.clone(),
        auth_guard: auth_guard.clone(),
        health_state: health_state.clone(),
        pg_pool: &pg_pool,
        kevy_conn: &kevy_conn,
        outbound_queue: &outbound_queue,
        mailbox_store: &mailbox_store,
        domain_store: &domain_store,
        llm_provider: &llm_provider,
        resolver: &resolver,
        ldap_config: &ldap_config,
        meili_client: meili_client.as_ref(),
        system_config_store: system_config_store.clone(),
        metrics_handle: metrics_handle.clone(),
    }));

    // spawn meilisearch indexer
    if let (Some(meili), Some(pool)) = (&meili_client, &pg_pool) {
        search_index::spawn_indexer(meili.clone(), pool.clone());
        tracing::info!(event = "subsystem_started", subsystem = "meili_indexer");
    }

    // MRS-10: spawn external ICS feed worker. Cheap when no feeds exist —
    // the DUE query returns empty and the loop sleeps.
    if let Some(ref pool) = pg_pool {
        calendar::feed_worker::spawn_feed_worker(pool.clone());
        tracing::info!(event = "subsystem_started", subsystem = "external_ics_feed");
    }

    let users = Arc::new(user_store);

    let inbound_pipeline = build_inbound_pipeline_with_shadows(
        &greylist_db,
        &greylist_config,
        &resolver,
        &dmarc_report_store,
        &cfg,
        &llm_provider,
        &kevy_conn,
    );

    let ctx = Arc::new(ConnectionContext {
        hostname: cfg.hostname.clone(),
        maildir_root: cfg.maildir_root.clone(),
        tls_state: tls_state.clone(),
        users: users.clone(),
        event_bus: event_bus.clone(),
        web_state: web_state.clone(),
        rate_limiter,
        local_domains: cfg.local_domains.clone(),
        outbound_queue: outbound_queue.clone(),
        resolver,
        dnsbl_zones: cfg.dnsbl_zones.clone(),
        dnsbl_enabled: cfg.dnsbl_enabled,
        antispam_enabled: cfg.antispam_enabled,
        mailbox_store: mailbox_store.clone(),
        smuggle_protection: cfg.smuggle_protection,
        auth_guard: auth_guard.clone(),
        domain_store: domain_store.clone(),
        kevy: kevy_conn.clone(),
        srs_secret: cfg.srs_secret.clone(),
        ldap_config: ldap_config.clone(),
        inbound_pipeline,
        delivery_executor: mailrs_delivery_executor::DeliveryExecutor::spawn(),
    });

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
        ctx.outbound_queue.clone(),
        shutdown_rx.clone(),
    );

    spawn_rbl_monitor(&ctx.resolver, &cfg.hostname, &kevy_conn);

    // keep main alive
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for ctrl+c");
    tracing::info!("shutting down");
    let _ = shutdown_tx.send(true);
}
