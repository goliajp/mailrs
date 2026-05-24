mod acme;
mod ai_analyzer;
mod api_key_store;
mod config;

mod content_worker;
mod conversation_cache;

mod dmarc_report;
mod domain_store;
pub(crate) mod permission;
mod event_bus;
mod fbl;
mod health;
mod calendar;

mod ldap_auth;

mod imap_session;
pub mod inbound;
mod inline_image;
mod message_util;
mod pg;
mod managesieve_session;
mod pop3_session;
mod rbl_monitor;
mod render_preview;
mod reputation;
mod search_index;
mod listeners;
mod outbound_tls_rpt;

mod smtp_session;
pub(crate) mod system_config;
mod tls;
mod totp;
mod users;
mod valkey_store;
mod mcp;
mod oidc_jwt;
mod oidc_store;
mod web;
mod webhook;

use std::sync::Arc;

use hickory_resolver::TokioResolver;
use tokio::net::TcpListener;

use crate::config::{ServerConfig, TlsMode};
use crate::event_bus::{EventBus, SmtpEvent};
use crate::inbound::auth_guard::{AuthGuard, AuthGuardConfig};
use mailrs_shield::greylist::{GreylistConfig, GreylistDb};
use crate::inbound::rate_limit::{RateLimiter, TokenBucketConfig};
use crate::smtp_session::ConnectionContext;
use crate::users::UserStore;
use crate::web::WebState;
use mailrs_mailbox::PgMailboxStore;

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

    let cfg = ServerConfig::from_env();

    for warning in cfg.validate() {
        tracing::warn!(warning, "config warning");
    }

    let domains_str = if cfg.local_domains.is_empty() { "(none)".into() } else { cfg.local_domains.join(", ") };
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

    // PG + Valkey connections (optional, graceful degradation)
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

    let valkey_conn = match &cfg.valkey_url {
        Some(url) => match valkey_store::create_connection(url).await {
            Ok(conn) => {
                tracing::info!("valkey connected");
                Some(conn)
            }
            Err(e) => {
                tracing::warn!(error = %e, "valkey connection failed, running in degraded mode");
                None
            }
        },
        None => None,
    };

    let health_state = health::HealthState::new();
    if let (Some(pg), Some(vk)) = (&pg_pool, &valkey_conn) {
        health::spawn_health_checker(pg.clone(), vk.clone(), health_state.clone());
        health_state.set_pg(true);
        health_state.set_valkey(true);
    }

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let tls_state = init_tls_state(&cfg, shutdown_rx.clone()).await;

    let user_store = match &cfg.users_file {
        Some(path) => UserStore::load(path).expect("failed to load users file"),
        None => UserStore::empty(),
    };

    let event_bus = EventBus::new(1024);

    spawn_cache_bust_task(&valkey_conn, &event_bus);

    let rate_limiter = Arc::new(RateLimiter::new(TokenBucketConfig {
        capacity: cfg.rate_limit_capacity,
        refill_rate: cfg.rate_limit_refill,
    }));

    let outbound_queue = pg_pool.clone();

    // DNS resolver for DNSBL and other lookups
    let resolver = TokioResolver::builder_tokio()
        .ok()
        .and_then(|b| b.build().ok())
        .map(Arc::new);

    // PTR record check
    if let Some(ref r) = resolver {
        mailrs_shield::ptr::check_ptr_record(r, &cfg.hostname).await;
    }

    // greylisting (Valkey primary + PG cold backup)
    let greylist_db = valkey_conn.as_ref().map(|vk| {
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

    // mail authenticator (SPF/DKIM/DMARC/ARC)
    let mail_authenticator = if cfg.antispam_enabled {
        match mail_auth::MessageAuthenticator::new_system_conf() {
            Ok(a) => Some(Arc::new(a)),
            Err(e) => {
                eprintln!("warning: mail authenticator init failed: {e}");
                None
            }
        }
    } else {
        None
    };

    let auth_guard = init_auth_guard(&cfg);

    // mailbox store for IMAP (PG-backed)
    let mailbox_store = pg_pool
        .as_ref()
        .map(|pool| Arc::new(PgMailboxStore::new(pool.clone())));

    // domain store (PG + Valkey + process cache)
    let domain_store = if pg_pool.is_some() {
        let ds = Arc::new(domain_store::DomainStore::new(
            pg_pool.clone(),
            valkey_conn.clone(),
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
        && let Err(e) = oidc_jwt::ensure_signing_key(pool).await {
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
            eprintln!("backfilled threading data for {count} messages");
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
            Some(Arc::new(mailrs_intelligence::OpenAiCompatibleProvider::new(
                cfg.llm_url.clone(),
                cfg.llm_api_key.clone(),
                model_id,
            )))
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
        init_system_config_store(&cfg, &pg_pool, &valkey_conn, shutdown_rx.clone()).await;

    let web_state = Arc::new(build_web_state(WebStateInputs {
        cfg: &cfg,
        event_bus: event_bus.clone(),
        auth_guard: auth_guard.clone(),
        health_state: health_state.clone(),
        pg_pool: &pg_pool,
        valkey_conn: &valkey_conn,
        outbound_queue: &outbound_queue,
        mailbox_store: &mailbox_store,
        domain_store: &domain_store,
        llm_provider: &llm_provider,
        resolver: &resolver,
        ldap_config: &ldap_config,
        meili_client: meili_client.as_ref(),
        system_config_store: system_config_store.clone(),
    }));

    // spawn meilisearch indexer
    if let (Some(meili), Some(pool)) = (&meili_client, &pg_pool) {
        search_index::spawn_indexer(meili.clone(), pool.clone());
        eprintln!("Meilisearch indexer started");
    }

    // MRS-10: spawn external ICS feed worker. Cheap when no feeds exist —
    // the DUE query returns empty and the loop sleeps.
    if let Some(ref pool) = pg_pool {
        calendar::feed_worker::spawn_feed_worker(pool.clone());
        eprintln!("External ICS feed worker started");
    }

    let users = Arc::new(user_store);

    let inbound_pipeline = build_inbound_pipeline_with_shadows(
        &greylist_db,
        &greylist_config,
        &resolver,
        &mail_authenticator,
        &dmarc_report_store,
        &cfg,
        &llm_provider,
        &valkey_conn,
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
        mail_authenticator,
        mailbox_store: mailbox_store.clone(),
        smuggle_protection: cfg.smuggle_protection,
        auth_guard: auth_guard.clone(),
        domain_store: domain_store.clone(),
        valkey: valkey_conn.clone(),
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

    spawn_rbl_monitor(&ctx.resolver, &cfg.hostname, &valkey_conn);

    // keep main alive
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for ctrl+c");
    tracing::info!("shutting down");
    let _ = shutdown_tx.send(true);
}

/// Bind the web HTTP listener, spawn the session-cleanup task,
/// spawn the domain-store cache-eviction task (60s interval), and
/// spawn the axum serve task with graceful shutdown wired to the
/// shared `shutdown_rx`.
async fn spawn_web_server(
    web_state: Arc<WebState>,
    cfg: &config::ServerConfig,
    domain_store: &Option<Arc<domain_store::DomainStore>>,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    let web_addr = format!("0.0.0.0:{}", cfg.web_port);
    let web_listener = TcpListener::bind(&web_addr)
        .await
        .expect("failed to bind web port");
    tracing::info!(addr = web_addr.as_str(), "web API listening");

    let static_dir = cfg
        .web_static_dir
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned());

    web::spawn_session_cleanup(web_state.clone());

    if let Some(ds) = domain_store {
        let ds_cleanup = ds.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                let evicted = ds_cleanup.evict_expired();
                if evicted > 0 {
                    eprintln!("domain cache: evicted {evicted} expired entries");
                }
            }
        });
    }

    let app = web::router(web_state, static_dir.as_deref());
    let web_shutdown = shutdown_rx;
    tokio::spawn(async move {
        axum::serve(
            web_listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            let mut rx = web_shutdown;
            while rx.changed().await.is_ok() {
                if *rx.borrow() {
                    break;
                }
            }
        })
        .await
        .ok();
    });
}

/// Spawn three webhook-related background tasks:
///   1. Global webhook — fire-and-forget POST on every event,
///      URL pulled from the runtime-editable system config.
///   2. PG webhook listener — subscribes to PG NOTIFY channels
///      so a `mailrs webhook fire` from another process triggers
///      delivery.
///   3. Webhook delivery worker — drains the webhook queue with
///      retry/backoff.
///
/// Items 2 and 3 need a PG pool; item 1 is independent.
fn spawn_webhook_subsystem(
    pg_pool: &Option<sqlx::PgPool>,
    event_bus: &EventBus,
    system_config_store: &Arc<system_config::SystemConfigStore>,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    {
        let eb = event_bus.clone();
        let store = system_config_store.clone();
        let rx = shutdown_rx.clone();
        tokio::spawn(async move {
            webhook::global::run(&eb, store, rx).await;
        });
        eprintln!("global webhook enabled");
    }

    if let Some(pool) = pg_pool {
        let pool_clone = pool.clone();
        let eb = event_bus.clone();
        let rx = shutdown_rx.clone();
        tokio::spawn(async move {
            webhook::listener::run(&eb, &pool_clone, rx).await;
        });

        let worker = webhook::worker::WebhookWorker::new(pool.clone());
        let rx = shutdown_rx;
        tokio::spawn(async move {
            worker.run(rx).await;
        });
        eprintln!("mailrs webhook system started");
    }
}

/// Spawn the daily DMARC aggregate-report builder + submitter.
/// Reads per-message DMARC outcomes the inbound pipeline
/// recorded, batches per-domain rua reports, sends via the
/// outbound queue addressed to `postmaster@<hostname>`.
/// No-op without a DMARC report store or DNS resolver.
fn spawn_dmarc_aggregate_task(
    dmarc_report_store: &Option<Arc<dmarc_report::DmarcReportStore>>,
    resolver: &Option<Arc<hickory_resolver::TokioResolver>>,
    cfg: &config::ServerConfig,
    outbound_queue: Option<sqlx::PgPool>,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    let (Some(dmarc_store), Some(resolver)) = (dmarc_report_store, resolver) else {
        return;
    };
    dmarc_report::spawn_daily_report_task(
        dmarc_store.clone(),
        cfg.hostname.clone(),
        format!("postmaster@{}", cfg.hostname),
        cfg.hostname.clone(),
        resolver.clone(),
        outbound_queue,
        shutdown_rx,
    );
    eprintln!("DMARC report generation enabled");
}

/// Spawn the RBL (DNS blocklist) self-monitor — periodically
/// checks whether our hostname is listed on common RBLs and logs
/// a warning if so. Helps operators notice reputation hits before
/// outbound delivery starts bouncing.
fn spawn_rbl_monitor(
    resolver: &Option<Arc<hickory_resolver::TokioResolver>>,
    hostname: &str,
    valkey_conn: &Option<redis::aio::ConnectionManager>,
) {
    let Some(resolver) = resolver else { return };
    rbl_monitor::start(resolver.clone(), hostname.to_string(), valkey_conn.clone());
    eprintln!("RBL blocklist monitor started");
}

/// Subscribe to `SmtpEvent::NewMessage` and drop the Valkey cache
/// for the recipient's conversation list / categories /
/// action-count + the affected thread. Server + frontend caches
/// stay coherent: WS NewMessage triggers RQ invalidate on the
/// client; this task does the equivalent for the server cache so
/// the next read goes back to PG and picks up the new message.
///
/// No-op when Valkey isn't configured (no cache to bust).
fn spawn_cache_bust_task(
    valkey_conn: &Option<redis::aio::ConnectionManager>,
    event_bus: &EventBus,
) {
    let Some(vk) = valkey_conn else { return };
    let vk = vk.clone();
    let mut rx = event_bus.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event_bus::SmtpEvent::NewMessage {
                    user, thread_id, ..
                }) => {
                    conversation_cache::bust_thread(&vk, &user, &thread_id).await;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                _ => {}
            }
        }
    });
}

/// Construct the brute-force `AuthGuard` from per-account + per-IP
/// thresholds in `cfg`, and spawn a 5-minute periodic cleanup task
/// that evicts entries past their lockout window.
fn init_auth_guard(cfg: &config::ServerConfig) -> Arc<AuthGuard> {
    let auth_guard = Arc::new(AuthGuard::new(AuthGuardConfig {
        max_failures_account: cfg.auth_max_failures_account,
        account_window_secs: cfg.auth_account_window_secs,
        base_lockout_secs: cfg.auth_base_lockout_secs,
        max_failures_ip: cfg.auth_max_failures_ip,
        ip_window_secs: cfg.auth_ip_window_secs,
        ip_base_lockout_secs: cfg.auth_ip_base_lockout_secs,
        backoff_multiplier: cfg.auth_backoff_multiplier,
        max_lockout_secs: cfg.auth_max_lockout_secs,
    }));
    let auth_guard_cleanup = auth_guard.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        loop {
            interval.tick().await;
            auth_guard_cleanup.cleanup_stale(std::time::Instant::now());
        }
    });
    auth_guard
}

/// Initialize TLS state per config:
///
/// - ACME (Let's Encrypt): issues + renews certs, spawns the
///   HTTP-01 challenge responder on `:80`.
/// - Manual: loads the cert + key paths from disk.
/// - None: TLS disabled (STARTTLS unavailable too).
async fn init_tls_state(
    cfg: &config::ServerConfig,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> Option<tls::TlsState> {
    match cfg.tls_mode() {
        TlsMode::Acme => {
            let email = cfg
                .acme_email
                .as_ref()
                .expect("MAILRS_ACME_EMAIL must be set when TLS mode is ACME");
            let domains = &cfg.acme_domains;
            if domains.is_empty() {
                panic!("MAILRS_ACME_EMAIL is set but MAILRS_ACME_DOMAINS is empty");
            }

            // challenge tokens shared between ACME init and challenge server
            let challenge_tokens: acme::ChallengeTokens = Default::default();
            use std::net::SocketAddr;
            let challenge_addr: SocketAddr = ([0, 0, 0, 0], 80).into();
            acme::spawn_challenge_server(
                challenge_tokens.clone(),
                challenge_addr,
                shutdown_rx.clone(),
            );

            let (tls, account) = acme::init(
                email,
                domains,
                &cfg.acme_dir,
                cfg.acme_staging,
                &challenge_tokens,
            )
            .await
            .expect("failed to initialize ACME");

            acme::spawn_renewal_task(
                account,
                challenge_tokens,
                tls.clone(),
                acme::RenewalConfig {
                    domains: domains.clone(),
                    acme_dir: cfg.acme_dir.clone(),
                    ..Default::default()
                },
                shutdown_rx,
            );

            Some(tls)
        }
        TlsMode::Manual => {
            let tls_config = tls::load_tls_config(
                cfg.tls_cert
                    .as_ref()
                    .expect("MAILRS_TLS_CERT must be set when TLS mode is Manual"),
                cfg.tls_key
                    .as_ref()
                    .expect("MAILRS_TLS_KEY must be set when TLS mode is Manual"),
            )
            .expect("failed to load TLS certificate and key files");
            Some(tls::TlsState::new(
                std::sync::Arc::try_unwrap(tls_config).unwrap_or_else(|arc| (*arc).clone()),
            ))
        }
        TlsMode::None => None,
    }
}

/// Initialize the runtime-editable system config store, hydrate
/// from PG if available, and spawn the background reload task
/// that picks up DB changes without a restart.
async fn init_system_config_store(
    cfg: &config::ServerConfig,
    pg_pool: &Option<sqlx::PgPool>,
    valkey_conn: &Option<redis::aio::ConnectionManager>,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> Arc<system_config::SystemConfigStore> {
    let env_defaults = system_config::RuntimeConfig::from_server_config(cfg);
    let store = Arc::new(system_config::SystemConfigStore::new(
        pg_pool.clone(),
        valkey_conn.clone(),
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

/// Inputs to [`build_web_state`] — bundling them in a struct
/// avoids a 14-argument fn signature and makes the call site
/// readable. All fields are borrows of values already live in
/// `main` at the build point.
struct WebStateInputs<'a> {
    cfg: &'a config::ServerConfig,
    event_bus: EventBus,
    auth_guard: Arc<AuthGuard>,
    health_state: health::HealthState,
    pg_pool: &'a Option<sqlx::PgPool>,
    valkey_conn: &'a Option<redis::aio::ConnectionManager>,
    outbound_queue: &'a Option<sqlx::PgPool>,
    mailbox_store: &'a Option<Arc<PgMailboxStore>>,
    domain_store: &'a Option<Arc<domain_store::DomainStore>>,
    llm_provider: &'a Option<Arc<dyn mailrs_intelligence::provider::LlmProvider>>,
    resolver: &'a Option<Arc<hickory_resolver::TokioResolver>>,
    ldap_config: &'a Option<Arc<crate::ldap_auth::LdapConfig>>,
    meili_client: Option<&'a Arc<search_index::MeiliClient>>,
    system_config_store: Arc<system_config::SystemConfigStore>,
}

/// Build the `WebState` from optional backends. Optional pieces
/// (PG, Valkey, mailbox, domain store, LLM, resolver, LDAP,
/// Meilisearch, Chrome render preview, OIDC client) only attach
/// when the corresponding backend was successfully initialized.
fn build_web_state(i: WebStateInputs<'_>) -> WebState {
    let smtp_snapshot = crate::web::SmtpConfigSnapshot {
        hostname: i.cfg.hostname.clone(),
        smtp_port: i.cfg.smtp_port,
        submission_port: i.cfg.submission_port,
        imap_port: i.cfg.imap_port,
        local_domains: i.cfg.local_domains.clone(),
        max_message_size: None,
        tls_enabled: i.cfg.has_tls() || i.cfg.acme_email.is_some(),
    };

    let mut ws = WebState::new(i.event_bus)
        .with_maildir_root(i.cfg.maildir_root.clone())
        .with_hostname(i.cfg.hostname.clone())
        .with_auth_guard(i.auth_guard)
        .with_health(i.health_state)
        .with_smtp_config(smtp_snapshot)
        .with_system_config(i.system_config_store);

    if let Some(pool) = i.pg_pool {
        ws = ws.with_pg(pool.clone());
    }
    if let Some(vk) = i.valkey_conn {
        ws = ws.with_valkey(vk.clone());
    }
    if let Some(q) = i.outbound_queue {
        ws = ws.with_queue(q.clone());
    }
    if let Some(mb) = i.mailbox_store {
        ws = ws.with_mailbox(mb.clone());
    }
    if let Some(ds) = i.domain_store {
        ws = ws.with_domain_store(ds.clone());
    }
    if let Some(ref mode) = i.cfg.mta_sts_mode {
        ws = ws.with_mta_sts(
            mode.clone(),
            i.cfg.mta_sts_mx.clone(),
            i.cfg.mta_sts_max_age,
            i.cfg.mta_sts_id.clone(),
        );
    }
    if let Some(provider) = i.llm_provider {
        ws = ws.with_llm(provider.clone());
    }
    if let Some(r) = i.resolver {
        ws = ws.with_resolver(r.clone());
    }
    if let Some(ref sel) = i.cfg.dkim_selector {
        ws = ws.with_dkim_selector(sel.clone());
    }
    if let Some(ldap) = i.ldap_config {
        ws = ws.with_ldap_config(ldap.clone());
    }
    if let Some(ref url) = i.cfg.chrome_cdp_url {
        let client = Arc::new(render_preview::RenderPreviewClient::new(url.clone(), 5));
        ws = ws.with_render_preview(client);
        eprintln!("Email render preview enabled (Chrome CDP: {url})");
    }
    if let Some(meili) = i.meili_client {
        ws = ws.with_meili(meili.clone());
    }
    if let Some(oidc) = oidc_client_from_env(&i.cfg.hostname) {
        tracing::info!("OIDC client configured (issuer={})", oidc.token_url);
        ws = ws.with_oidc(oidc);
    }
    ws
}

/// Read `MAILRS_OIDC_*` env vars and build the external-IdP
/// "Sign in with X" config. Returns None unless the three
/// required vars (CLIENT_ID, CLIENT_SECRET, ISSUER) are all set;
/// derives optional URLs from `ISSUER` if not overridden.
fn oidc_client_from_env(hostname: &str) -> Option<crate::web::OidcConfig> {
    let client_id = std::env::var("MAILRS_OIDC_CLIENT_ID").ok()?;
    let client_secret = std::env::var("MAILRS_OIDC_CLIENT_SECRET").ok()?;
    let issuer = std::env::var("MAILRS_OIDC_ISSUER").ok()?;
    let redirect_uri = std::env::var("MAILRS_OIDC_REDIRECT_URI")
        .unwrap_or_else(|_| format!("https://{hostname}/api/auth/oidc/callback"));
    let authorize_url = std::env::var("MAILRS_OIDC_AUTHORIZE_URL")
        .unwrap_or_else(|_| format!("{issuer}/authorize"));
    let token_url =
        std::env::var("MAILRS_OIDC_TOKEN_URL").unwrap_or_else(|_| format!("{issuer}/token"));
    let userinfo_url = std::env::var("MAILRS_OIDC_USERINFO_URL")
        .unwrap_or_else(|_| format!("{issuer}/userinfo"));
    Some(crate::web::OidcConfig {
        client_id,
        client_secret,
        authorize_url,
        token_url,
        userinfo_url,
        redirect_uri,
    })
}

/// Spawn the three SMTP-family listeners that all dispatch into
/// the shared `ConnectionContext`:
///   - port `smtp_port` (25/2525) — plain SMTP, STARTTLS optional
///   - port `submission_port` (587/2587) — message submission
///   - port `smtps_port` (465/2465) — implicit-TLS submission
///     (skipped if no TLS configured)
async fn spawn_smtp_listeners(
    ctx: &Arc<ConnectionContext>,
    cfg: &config::ServerConfig,
    tls_configured: bool,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    let ctx_smtp = ctx.clone();
    listeners::spawn_plain(
        format!("0.0.0.0:{}", cfg.smtp_port),
        "smtp",
        shutdown_rx.clone(),
        move |stream, addr| {
            let ctx = ctx_smtp.clone();
            async move { smtp_session::handle_plain_connection(stream, addr, ctx).await }
        },
    )
    .await;

    let ctx_sub = ctx.clone();
    listeners::spawn_plain(
        format!("0.0.0.0:{}", cfg.submission_port),
        "submission",
        shutdown_rx.clone(),
        move |stream, addr| {
            let ctx = ctx_sub.clone();
            async move { smtp_session::handle_plain_connection(stream, addr, ctx).await }
        },
    )
    .await;

    if tls_configured {
        let ctx_tls = ctx.clone();
        listeners::spawn_plain(
            format!("0.0.0.0:{}", cfg.smtps_port),
            "smtps",
            shutdown_rx,
            move |stream, addr| {
                let ctx = ctx_tls.clone();
                async move { smtp_session::handle_tls_connection(stream, addr, ctx).await }
            },
        )
        .await;
    }
}

/// Spawn IMAP plain (port 143/1143) and IMAPS implicit-TLS
/// (port 993). Both are no-ops without a mailbox_store; IMAPS
/// additionally requires `tls_state`. Each connection runs
/// `imap_session::handle_connection` with the same shared state.
#[allow(clippy::too_many_arguments)]
async fn spawn_imap_listeners(
    mailbox_store: &Option<Arc<PgMailboxStore>>,
    users: &Arc<crate::users::UserStore>,
    auth_guard: &Arc<AuthGuard>,
    domain_store: &Option<Arc<domain_store::DomainStore>>,
    event_bus: &EventBus,
    ldap_config: &Option<Arc<crate::ldap_auth::LdapConfig>>,
    tls_state: &Option<tls::TlsState>,
    cfg: &config::ServerConfig,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    if let Some(mb_store) = mailbox_store.as_ref().cloned() {
        let imap_users = users.clone();
        let imap_hostname = cfg.hostname.clone();
        let imap_maildir_root = cfg.maildir_root.clone();
        let imap_auth_guard = auth_guard.clone();
        let imap_domain_store = domain_store.clone();
        let imap_event_bus = event_bus.clone();
        let imap_ldap = ldap_config.clone();
        listeners::spawn_plain(
            format!("0.0.0.0:{}", cfg.imap_port),
            "imap",
            shutdown_rx.clone(),
            move |stream, addr| {
                let mb = mb_store.clone();
                let u = imap_users.clone();
                let h = imap_hostname.clone();
                let mr = imap_maildir_root.clone();
                let ag = imap_auth_guard.clone();
                let ds = imap_domain_store.clone();
                let eb = imap_event_bus.clone();
                let ldap = imap_ldap.clone();
                async move {
                    imap_session::handle_connection(stream, addr, mb, u, ag, ds, ldap, eb, &h, &mr).await;
                }
            },
        )
        .await;
    }

    if let (Some(mb_store), Some(imaps_tls)) =
        (mailbox_store.as_ref().cloned(), tls_state.clone())
    {
        let imaps_users = users.clone();
        let imaps_hostname = cfg.hostname.clone();
        let imaps_maildir_root = cfg.maildir_root.clone();
        let imaps_auth_guard = auth_guard.clone();
        let imaps_domain_store = domain_store.clone();
        let imaps_event_bus = event_bus.clone();
        let imaps_ldap = ldap_config.clone();
        listeners::spawn_plain(
            format!("0.0.0.0:{}", cfg.imaps_port),
            "imaps",
            shutdown_rx,
            move |stream, addr| {
                let tls = imaps_tls.clone();
                let mb = mb_store.clone();
                let u = imaps_users.clone();
                let h = imaps_hostname.clone();
                let mr = imaps_maildir_root.clone();
                let ag = imaps_auth_guard.clone();
                let ds = imaps_domain_store.clone();
                let eb = imaps_event_bus.clone();
                let ldap = imaps_ldap.clone();
                async move {
                    match tls.acceptor().accept(stream).await {
                        Ok(tls_stream) => {
                            imap_session::handle_connection(
                                tls_stream, addr, mb, u, ag, ds, ldap, eb, &h, &mr,
                            )
                            .await;
                        }
                        Err(e) => {
                            tracing::error!(?addr, error = %e, "imaps tls handshake error");
                        }
                    }
                }
            },
        )
        .await;
    }
}

/// Spawn the POP3 listener (port 110/1110 etc per config). No-op
/// when mailbox_store is None (PG unavailable → POP3 has nothing
/// to serve).
async fn spawn_pop3_listener(
    mailbox_store: &Option<Arc<PgMailboxStore>>,
    users: &Arc<crate::users::UserStore>,
    auth_guard: &Arc<AuthGuard>,
    domain_store: &Option<Arc<domain_store::DomainStore>>,
    ldap_config: &Option<Arc<crate::ldap_auth::LdapConfig>>,
    cfg: &config::ServerConfig,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    let Some(mb_store) = mailbox_store.as_ref().cloned() else {
        return;
    };
    let pop3_users = users.clone();
    let pop3_maildir_root = cfg.maildir_root.clone();
    let pop3_auth_guard = auth_guard.clone();
    let pop3_domain_store = domain_store.clone();
    let pop3_ldap = ldap_config.clone();
    listeners::spawn_plain(
        format!("0.0.0.0:{}", cfg.pop3_port),
        "pop3",
        shutdown_rx,
        move |stream, addr| {
            let mb = mb_store.clone();
            let u = pop3_users.clone();
            let mr = pop3_maildir_root.clone();
            let ag = pop3_auth_guard.clone();
            let ds = pop3_domain_store.clone();
            let ldap = pop3_ldap.clone();
            async move {
                pop3_session::handle_connection(stream, addr, mb, u, ag, ds, ldap, &mr).await;
            }
        },
    )
    .await;
}

/// Spawn the ManageSieve listener (RFC 5804) — port 4190 etc per
/// config. Always spawned; doesn't depend on PG.
async fn spawn_managesieve_listener(
    users: &Arc<crate::users::UserStore>,
    auth_guard: &Arc<AuthGuard>,
    domain_store: &Option<Arc<domain_store::DomainStore>>,
    ldap_config: &Option<Arc<crate::ldap_auth::LdapConfig>>,
    cfg: &config::ServerConfig,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    let sieve_users = users.clone();
    let sieve_auth_guard = auth_guard.clone();
    let sieve_domain_store = domain_store.clone();
    let sieve_ldap = ldap_config.clone();
    listeners::spawn_plain(
        format!("0.0.0.0:{}", cfg.managesieve_port),
        "managesieve",
        shutdown_rx,
        move |stream, addr| {
            let u = sieve_users.clone();
            let ag = sieve_auth_guard.clone();
            let ds = sieve_domain_store.clone();
            let ldap = sieve_ldap.clone();
            async move {
                managesieve_session::handle_connection(stream, addr, u, ag, ds, ldap).await;
            }
        },
    )
    .await;
}

/// Build the inbound pipeline + the four shadow-mode resolvers
/// (SPF / DKIM / ARC / DMARC) used to validate our in-house
/// `mailrs-*` crates against `mail-auth` on real prod traffic.
/// Each shadow runs in parallel; comparison happens inside
/// `MailAuthStage`. NO impact on production decisions.
#[allow(clippy::too_many_arguments)]
fn build_inbound_pipeline_with_shadows(
    greylist_db: &Option<Arc<mailrs_shield::greylist::GreylistDb>>,
    greylist_config: &mailrs_shield::greylist::GreylistConfig,
    resolver: &Option<Arc<hickory_resolver::TokioResolver>>,
    mail_authenticator: &Option<Arc<mail_auth::MessageAuthenticator>>,
    dmarc_report_store: &Option<Arc<dmarc_report::DmarcReportStore>>,
    cfg: &config::ServerConfig,
    llm_provider: &Option<Arc<dyn mailrs_intelligence::provider::LlmProvider>>,
    valkey_conn: &Option<redis::aio::ConnectionManager>,
) -> mailrs_inbound::Pipeline {
    let shadow_spf_resolver = resolver
        .as_ref()
        .map(|r| Arc::new(mailrs_spf::HickoryResolver::new((**r).clone())));
    let shadow_dkim_resolver = resolver
        .as_ref()
        .map(|r| Arc::new(mailrs_dkim::HickoryDkimResolver::new((**r).clone())));
    // ARC reuses the DKIM resolver (ArcResolver = DkimResolver).
    // One hickory bind, two stones.
    let shadow_arc_resolver = shadow_dkim_resolver.clone();
    // DMARC's shadow path needs raw TXT lookup against the
    // hickory TokioResolver (mailrs-dmarc itself is a pure
    // evaluator with no DNS). Same resolver mail-auth uses.
    let shadow_dmarc_resolver = resolver.clone();

    crate::inbound::pipeline::build_inbound_pipeline(
        greylist_db.clone(),
        greylist_config.clone(),
        resolver.clone(),
        mail_authenticator.clone(),
        dmarc_report_store.clone(),
        shadow_spf_resolver,
        shadow_dkim_resolver,
        shadow_arc_resolver,
        shadow_dmarc_resolver,
        cfg.clamav_addr.clone(),
        llm_provider.clone(),
        valkey_conn.clone(),
        cfg.spam_score_threshold,
    )
}

/// Spawn the outbound `DeliveryWorker` and its 24h TLSRPT
/// flush companion task. Configures DKIM signing if the cfg
/// has selector/domain/key set; bridges DeliveryEvent into the
/// SmtpEvent bus; and persists per-attempt TLS outcomes to a
/// PG-backed TLSRPT store so the daily flush survives restart.
///
/// No-op if `outbound_queue` is None (PG unavailable) or
/// `resolver` is None (DNS unavailable) — in either case
/// delivery would fail anyway.
fn spawn_outbound_delivery(
    outbound_queue: Option<&sqlx::PgPool>,
    resolver: Option<&Arc<hickory_resolver::TokioResolver>>,
    cfg: &config::ServerConfig,
    event_bus: EventBus,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    let Some(pool) = outbound_queue else { return };
    let Some(resolver) = resolver else {
        eprintln!(
            "warning: queue_db configured but no DNS resolver available, delivery worker disabled"
        );
        return;
    };

    let mut worker = build_delivery_worker(pool, resolver, cfg);
    let tls_rpt_obs = Arc::new(outbound_tls_rpt::TlsRptObserver::new(
        outbound_tls_rpt::PgTlsRptStore::new(pool.clone()).into_arc(),
    ));
    worker = worker.with_event_sender(make_delivery_event_sender(
        event_bus,
        tls_rpt_obs.clone(),
    ));

    spawn_tlsrpt_flush_task(
        tls_rpt_obs,
        cfg.hostname.clone(),
        resolver.clone(),
        pool.clone(),
    );

    let rx = shutdown_rx.clone();
    tokio::spawn(async move {
        worker.run(rx).await;
    });
    tracing::info!("delivery worker started");
}

/// Construct the outbound `DeliveryWorker` with the per-config
/// Valkey URL and DKIM signing key (if configured). Pure
/// construction — no spawning.
fn build_delivery_worker(
    pool: &sqlx::PgPool,
    resolver: &Arc<hickory_resolver::TokioResolver>,
    cfg: &config::ServerConfig,
) -> mailrs_outbound_queue::DeliveryWorker {
    let mut worker = mailrs_outbound_queue::DeliveryWorker::new(
        mailrs_outbound_queue::worker::WorkerConfig::default(),
        pool.clone(),
        (**resolver).clone(),
        cfg.hostname.clone(),
    );
    if let Some(ref url) = cfg.valkey_url {
        worker = worker.with_valkey(url.clone());
    }
    if let (Some(selector), Some(domain), Some(key_path)) = (
        &cfg.dkim_selector,
        &cfg.dkim_domain,
        &cfg.dkim_private_key_path,
    ) {
        match std::fs::read_to_string(key_path) {
            Ok(pem) => {
                worker = worker.with_dkim(mailrs_outbound_queue::DkimSignConfig {
                    selector: selector.clone(),
                    domain: domain.clone(),
                    private_key_pem: pem,
                });
                eprintln!("DKIM signing enabled (selector={selector}, domain={domain})");
            }
            Err(e) => {
                eprintln!(
                    "warning: failed to read DKIM key {}: {e}",
                    key_path.display()
                );
            }
        }
    }
    worker
}

/// Build the closure the `DeliveryWorker` calls on every
/// outbound event. Bridges:
///   - `DeliveryEvent::Attempt` → `SmtpEvent::DeliveryAttempt`
///   - `DeliveryEvent::TlsAttempt` → fire-and-forget into
///     `TlsRptObserver` (NOT emitted as `SmtpEvent` — TLS-level
///     events aren't on the web UI's surface yet).
///   - `DeliveryEvent::{Success, Failed, Bounced}` → matching
///     `SmtpEvent` variants emitted on the event bus.
fn make_delivery_event_sender(
    event_bus: EventBus,
    tls_rpt_obs: Arc<outbound_tls_rpt::TlsRptObserver>,
) -> Arc<dyn Fn(mailrs_outbound_queue::DeliveryEvent) + Send + Sync> {
    Arc::new(move |evt| {
        use mailrs_outbound_queue::DeliveryEvent;
        let tls_obs = tls_rpt_obs.clone();
        let smtp_evt = match evt {
            DeliveryEvent::Attempt { queue_id, domain } => {
                SmtpEvent::DeliveryAttempt { queue_id, domain }
            }
            DeliveryEvent::TlsAttempt {
                domain,
                mx_host,
                outcome,
            } => {
                tokio::spawn(async move {
                    tls_obs.record_tls_attempt(&domain, &mx_host, &outcome).await;
                });
                return;
            }
            DeliveryEvent::Success { queue_id, domain } => {
                SmtpEvent::DeliverySuccess { queue_id, domain }
            }
            DeliveryEvent::Failed {
                queue_id,
                domain,
                error,
            } => SmtpEvent::DeliveryFailed {
                queue_id,
                domain,
                error,
            },
            DeliveryEvent::Bounced { queue_id, sender } => {
                SmtpEvent::BounceGenerated { queue_id, sender }
            }
        };
        event_bus.emit(smtp_evt);
    })
}

/// Spawn the daily TLSRPT flush task — every 24h, drain the
/// accumulated window from the `TlsRptObserver` store, build
/// the RFC 8460 report, and submit to each rua endpoint
/// (mailto: via outbound queue, https: via reqwest POST).
fn spawn_tlsrpt_flush_task(
    tls_rpt_obs: Arc<outbound_tls_rpt::TlsRptObserver>,
    hostname: String,
    resolver: Arc<hickory_resolver::TokioResolver>,
    pool: sqlx::PgPool,
) {
    tokio::spawn(async move {
        // One reqwest client shared across windows. rustls, 30s
        // timeout, no redirects (RFC 8460 §6 says POST to the
        // literal URL).
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .ok();
        let mut tick = tokio::time::interval(tokio::time::Duration::from_secs(86_400));
        tick.tick().await; // skip the immediate first tick
        loop {
            tick.tick().await;
            let now = chrono::Utc::now();
            let start = now - chrono::Duration::hours(24);
            let report_id = format!("{}-tlsrpt-{}", hostname, now.format("%Y%m%d"));
            let submitter_address = format!("tlsrpt@{hostname}");
            let report_opt = match tls_rpt_obs
                .take_report(
                    start.timestamp().max(0) as u64,
                    now.timestamp().max(0) as u64,
                    &hostname,
                    &format!("mailto:{submitter_address}"),
                    &report_id,
                    &start.to_rfc3339(),
                    &now.to_rfc3339(),
                )
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(
                        event = "tls_rpt_take_report_failed",
                        error = %e,
                        report_id = %report_id,
                        "TLSRPT take_report failed — store backend error"
                    );
                    continue;
                }
            };
            if let Some(report) = report_opt {
                let (ok, failed) = outbound_tls_rpt::submit_report(
                    &report,
                    &hostname,
                    &submitter_address,
                    &resolver,
                    Some(&pool),
                    http.as_ref(),
                )
                .await;
                tracing::info!(
                    event = "tls_rpt_submission_summary",
                    policies = report.policies.len(),
                    endpoints_ok = ok,
                    endpoints_failed = failed,
                    report_id = %report_id,
                    "TLSRPT daily submission complete"
                );
            }
        }
    });
}
