mod acme;
mod ai_analyzer;
mod api_key_store;
mod codec;
mod config;
mod content_extract;
mod content_worker;
mod conversation_cache;
mod dmarc_report;
mod domain_store;
pub(crate) mod permission;
mod event_bus;
mod fbl;
mod health;
mod calendar;
mod imap_codec;
mod ldap_auth;
mod imap_format;
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
mod sieve;
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
use tokio::net::{TcpListener, TcpStream};

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

    // challenge tokens shared between ACME init and challenge server
    let challenge_tokens: acme::ChallengeTokens = Default::default();

    let tls_state = match cfg.tls_mode() {
        TlsMode::Acme => {
            let email = cfg.acme_email.as_ref()
                .expect("MAILRS_ACME_EMAIL must be set when TLS mode is ACME");
            let domains = &cfg.acme_domains;
            if domains.is_empty() {
                panic!("MAILRS_ACME_EMAIL is set but MAILRS_ACME_DOMAINS is empty");
            }

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
                challenge_tokens.clone(),
                tls.clone(),
                acme::RenewalConfig {
                    domains: domains.clone(),
                    acme_dir: cfg.acme_dir.clone(),
                    ..Default::default()
                },
                shutdown_rx.clone(),
            );

            Some(tls)
        }
        TlsMode::Manual => {
            let tls_config = tls::load_tls_config(
                cfg.tls_cert.as_ref()
                    .expect("MAILRS_TLS_CERT must be set when TLS mode is Manual"),
                cfg.tls_key.as_ref()
                    .expect("MAILRS_TLS_KEY must be set when TLS mode is Manual"),
            )
            .expect("failed to load TLS certificate and key files");
            Some(tls::TlsState::new(
                std::sync::Arc::try_unwrap(tls_config).unwrap_or_else(|arc| (*arc).clone()),
            ))
        }
        TlsMode::None => None,
    };

    let user_store = match &cfg.users_file {
        Some(path) => UserStore::load(path).expect("failed to load users file"),
        None => UserStore::empty(),
    };

    let event_bus = EventBus::new(1024);

    // Cache-bust task: when a new mail arrives, drop the Valkey cache for
    // that user's conversation list / categories / action-count, and the
    // affected thread, so the next read goes back to PG and picks up the
    // new message. Frontend RQ would refetch on its own (WS NewMessage →
    // invalidateQueries) so this keeps server and client caches consistent.
    if let Some(ref vk) = valkey_conn {
        let vk = vk.clone();
        let mut rx = event_bus.subscribe();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event_bus::SmtpEvent::NewMessage { user, thread_id, .. }) => {
                        conversation_cache::bust_thread(&vk, &user, &thread_id).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    _ => {}
                }
            }
        });
    }

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

    // auth guard (brute force protection)
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

    // spawn periodic cleanup
    let auth_guard_cleanup = auth_guard.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        loop {
            interval.tick().await;
            auth_guard_cleanup.cleanup_stale(std::time::Instant::now());
        }
    });

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

    let smtp_snapshot = crate::web::SmtpConfigSnapshot {
        hostname: cfg.hostname.clone(),
        smtp_port: cfg.smtp_port,
        submission_port: cfg.submission_port,
        imap_port: cfg.imap_port,
        local_domains: cfg.local_domains.clone(),
        max_message_size: None,
        tls_enabled: cfg.has_tls() || cfg.acme_email.is_some(),
    };

    let mut ws = WebState::new(event_bus.clone())
        .with_maildir_root(cfg.maildir_root.clone())
        .with_hostname(cfg.hostname.clone())
        .with_auth_guard(auth_guard.clone())
        .with_health(health_state.clone())
        .with_smtp_config(smtp_snapshot);
    if let Some(ref pool) = pg_pool {
        ws = ws.with_pg(pool.clone());
    }
    if let Some(ref vk) = valkey_conn {
        ws = ws.with_valkey(vk.clone());
    }
    if let Some(ref q) = outbound_queue {
        ws = ws.with_queue(q.clone());
    }
    if let Some(ref mb) = mailbox_store {
        ws = ws.with_mailbox(mb.clone());
    }
    if let Some(ref ds) = domain_store {
        ws = ws.with_domain_store(ds.clone());
    }
    if let Some(ref mode) = cfg.mta_sts_mode {
        ws = ws.with_mta_sts(
            mode.clone(),
            cfg.mta_sts_mx.clone(),
            cfg.mta_sts_max_age,
            cfg.mta_sts_id.clone(),
        );
    }
    if let Some(ref provider) = llm_provider {
        ws = ws.with_llm(provider.clone());
    }
    if let Some(ref r) = resolver {
        ws = ws.with_resolver(r.clone());
    }
    if let Some(ref sel) = cfg.dkim_selector {
        ws = ws.with_dkim_selector(sel.clone());
    }
    if let Some(ref ldap) = ldap_config {
        ws = ws.with_ldap_config(ldap.clone());
    }
    // Chrome CDP for email rendering preview
    if let Some(ref url) = cfg.chrome_cdp_url {
        let client = Arc::new(render_preview::RenderPreviewClient::new(url.clone(), 5));
        ws = ws.with_render_preview(client);
        eprintln!("Email render preview enabled (Chrome CDP: {url})");
    }
    // Meilisearch full-text search
    let meili_client = if let Some(ref url) = cfg.meili_url {
        let key = cfg.meili_key.clone().unwrap_or_default();
        let client = Arc::new(search_index::MeiliClient::new(url.clone(), key));
        ws = ws.with_meili(client.clone());
        Some(client)
    } else {
        None
    };
    // system config store (runtime-editable config from DB)
    let system_config_store = {
        let env_defaults = system_config::RuntimeConfig::from_server_config(&cfg);
        let store = Arc::new(system_config::SystemConfigStore::new(
            pg_pool.clone(),
            valkey_conn.clone(),
            env_defaults,
        ));
        if pg_pool.is_some()
            && let Err(e) = store.load_from_db().await {
                tracing::warn!("failed to load system config from DB: {e}");
            }
        let store_bg = store.clone();
        let rx = shutdown_rx.clone();
        tokio::spawn(async move {
            system_config::reload_task(store_bg, rx).await;
        });
        store
    };
    ws = ws.with_system_config(system_config_store.clone());

    // OIDC client (Sign in with external IdP)
    if let (Ok(client_id), Ok(client_secret), Ok(issuer)) = (
        std::env::var("MAILRS_OIDC_CLIENT_ID"),
        std::env::var("MAILRS_OIDC_CLIENT_SECRET"),
        std::env::var("MAILRS_OIDC_ISSUER"),
    ) {
        let redirect_uri = std::env::var("MAILRS_OIDC_REDIRECT_URI")
            .unwrap_or_else(|_| format!("https://{}/api/auth/oidc/callback", cfg.hostname));
        let authorize_url = std::env::var("MAILRS_OIDC_AUTHORIZE_URL")
            .unwrap_or_else(|_| format!("{issuer}/authorize"));
        let token_url = std::env::var("MAILRS_OIDC_TOKEN_URL")
            .unwrap_or_else(|_| format!("{issuer}/token"));
        let userinfo_url = std::env::var("MAILRS_OIDC_USERINFO_URL")
            .unwrap_or_else(|_| format!("{issuer}/userinfo"));
        tracing::info!("OIDC client configured (issuer={})", issuer);
        ws = ws.with_oidc(crate::web::OidcConfig {
            client_id,
            client_secret,
            authorize_url,
            token_url,
            userinfo_url,
            redirect_uri,
        });
    }
    let web_state = Arc::new(ws);

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

    // Shadow-mode SPF + DKIM: enable when resolver is up. The new
    // mailrs-* crates run in parallel to mail-auth, results compared
    // via tracing logs (info=match, warn=divergence). NO impact on
    // production decisions. Once divergence logs stay clean for a
    // sufficient period we cut over.
    let shadow_spf_resolver = resolver.as_ref().map(|r| {
        Arc::new(mailrs_spf::HickoryResolver::new((**r).clone()))
    });
    let shadow_dkim_resolver = resolver.as_ref().map(|r| {
        Arc::new(mailrs_dkim::HickoryDkimResolver::new((**r).clone()))
    });
    // ARC reuses the DKIM resolver (ArcResolver = DkimResolver). One
    // hickory bind, two stones.
    let shadow_arc_resolver = shadow_dkim_resolver.clone();
    // DMARC's shadow path needs raw TXT lookup against the hickory
    // TokioResolver (mailrs-dmarc itself is a pure evaluator with no
    // DNS). Same resolver mail-auth already uses.
    let shadow_dmarc_resolver = resolver.clone();

    let inbound_pipeline = crate::inbound::pipeline::build_inbound_pipeline(
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
    });

    // port 25/2525: plain SMTP (STARTTLS optional)
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

    // port 587/2587: submission (STARTTLS optional)
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

    // port 465/2465: implicit TLS (only if TLS configured)
    if tls_state.is_some() {
        let ctx_tls = ctx.clone();
        listeners::spawn_plain(
            format!("0.0.0.0:{}", cfg.smtps_port),
            "smtps",
            shutdown_rx.clone(),
            move |stream, addr| {
                let ctx = ctx_tls.clone();
                async move { smtp_session::handle_tls_connection(stream, addr, ctx).await }
            },
        )
        .await;
    }

    // web API + WebSocket
    let web_addr = format!("0.0.0.0:{}", cfg.web_port);
    let web_listener = TcpListener::bind(&web_addr)
        .await
        .expect("failed to bind web port");
    tracing::info!(addr = web_addr.as_str(), "web API listening");

    let static_dir = cfg
        .web_static_dir
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned());

    // spawn session cleanup task
    web::spawn_session_cleanup(web_state.clone());

    // spawn domain store cache eviction task
    if let Some(ref ds) = domain_store {
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
    let web_shutdown = shutdown_rx.clone();
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

    // IMAP listener (plain, port 143/1143)
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
                    handle_imap_connection(stream, addr, mb, u, ag, ds, ldap, eb, &h, &mr).await;
                }
            },
        )
        .await;
    }

    // IMAPS listener (implicit TLS, port 993)
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
            shutdown_rx.clone(),
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
                            handle_imap_connection(
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

    // POP3 listener
    if let Some(mb_store) = mailbox_store.as_ref().cloned() {
        let pop3_users = users.clone();
        let pop3_maildir_root = cfg.maildir_root.clone();
        let pop3_auth_guard = auth_guard.clone();
        let pop3_domain_store = domain_store.clone();
        let pop3_ldap = ldap_config.clone();
        listeners::spawn_plain(
            format!("0.0.0.0:{}", cfg.pop3_port),
            "pop3",
            shutdown_rx.clone(),
            move |stream, addr| {
                let mb = mb_store.clone();
                let u = pop3_users.clone();
                let mr = pop3_maildir_root.clone();
                let ag = pop3_auth_guard.clone();
                let ds = pop3_domain_store.clone();
                let ldap = pop3_ldap.clone();
                async move {
                    handle_pop3_connection(stream, addr, mb, u, ag, ds, ldap, &mr).await;
                }
            },
        )
        .await;
    }

    // ManageSieve listener (RFC 5804)
    {
        let sieve_users = users.clone();
        let sieve_auth_guard = auth_guard.clone();
        let sieve_domain_store = domain_store.clone();
        let sieve_ldap = ldap_config.clone();
        listeners::spawn_plain(
            format!("0.0.0.0:{}", cfg.managesieve_port),
            "managesieve",
            shutdown_rx.clone(),
            move |stream, addr| {
                let u = sieve_users.clone();
                let ag = sieve_auth_guard.clone();
                let ds = sieve_domain_store.clone();
                let ldap = sieve_ldap.clone();
                async move {
                    handle_managesieve_connection(stream, addr, u, ag, ds, ldap).await;
                }
            },
        )
        .await;
    }

    // delivery worker (outbound queue)
    if let Some(ref pool) = outbound_queue {
        if let Some(ref resolver) = ctx.resolver {
            let mut worker = mailrs_outbound_queue::DeliveryWorker::new(
                mailrs_outbound_queue::worker::WorkerConfig::default(),
                pool.clone(),
                (**resolver).clone(),
                cfg.hostname.clone(),
            );
            if let Some(ref url) = cfg.valkey_url {
                worker = worker.with_valkey(url.clone());
            }

            // configure DKIM signing if key is available
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

            // bridge outbound events to event bus + TLSRPT observer
            let eb = event_bus.clone();
            let tls_rpt_obs = outbound_tls_rpt::new_shared();
            let tls_rpt_for_handler = tls_rpt_obs.clone();
            worker = worker.with_event_sender(Arc::new(move |evt| {
                use mailrs_outbound_queue::DeliveryEvent;
                let tls_obs = tls_rpt_for_handler.clone();
                // TlsAttempt is the only event the TLSRPT observer
                // cares about. Success/Failed/Bounced carry only
                // queue-row metadata, which is web-UI territory.
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
                            tls_obs
                                .record_tls_attempt(&domain, &mx_host, &outcome)
                                .await;
                        });
                        // No SmtpEvent for TLS-level events yet (web
                        // UI doesn't surface them). Skip emitting.
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
                eb.emit(smtp_evt);
            }));

            // Periodic TLSRPT flush task — every 24h, build the
            // accumulated report and log as JSON. (Submission to rua
            // endpoints is a follow-up; this proves the data flow
            // works against real outbound traffic first.)
            let tls_rpt_flush = tls_rpt_obs.clone();
            let hostname_for_tls_rpt = cfg.hostname.clone();
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(tokio::time::Duration::from_secs(86_400));
                tick.tick().await; // skip the immediate first tick
                loop {
                    tick.tick().await;
                    let now = chrono::Utc::now();
                    let start = now - chrono::Duration::hours(24);
                    let report_id = format!(
                        "{}-tlsrpt-{}",
                        hostname_for_tls_rpt,
                        now.format("%Y%m%d")
                    );
                    if let Some(report) = tls_rpt_flush
                        .take_report(
                            &hostname_for_tls_rpt,
                            &format!("mailto:postmaster@{hostname_for_tls_rpt}"),
                            &report_id,
                            &start.to_rfc3339(),
                            &now.to_rfc3339(),
                        )
                        .await
                    {
                        match serde_json::to_string(&report) {
                            Ok(json) => tracing::info!(
                                event = "tls_rpt_report_built",
                                policies = report.policies.len(),
                                report_id = %report_id,
                                report = %json,
                                "TLSRPT daily report built (not yet submitted)"
                            ),
                            Err(e) => tracing::warn!(
                                event = "tls_rpt_report_serialize_error",
                                error = %e,
                                "TLSRPT serialize failed"
                            ),
                        }
                    }
                }
            });

            let rx = shutdown_rx.clone();
            tokio::spawn(async move {
                worker.run(rx).await;
            });
            tracing::info!("delivery worker started");
        } else {
            eprintln!("warning: queue_db configured but no DNS resolver available, delivery worker disabled");
        }
    }

    // global webhook (fire-and-forget POST on new mail, reads URL from system config store)
    {
        let eb = event_bus.clone();
        let store = system_config_store.clone();
        let rx = shutdown_rx.clone();
        tokio::spawn(async move {
            webhook::global::run(&eb, store, rx).await;
        });
        eprintln!("global webhook enabled");
    }

    // webhook listener + delivery worker
    if let Some(ref pool) = pg_pool {
        let pool_clone = pool.clone();
        let eb = event_bus.clone();
        let rx = shutdown_rx.clone();
        tokio::spawn(async move {
            webhook::listener::run(&eb, &pool_clone, rx).await;
        });

        let worker = webhook::worker::WebhookWorker::new(pool.clone());
        let rx = shutdown_rx.clone();
        tokio::spawn(async move {
            worker.run(rx).await;
        });
        eprintln!("mailrs webhook system started");
    }

    // DMARC aggregate report generation
    if let (Some(dmarc_store), Some(resolver)) = (&dmarc_report_store, &ctx.resolver) {
        dmarc_report::spawn_daily_report_task(
            dmarc_store.clone(),
            cfg.hostname.clone(),
            format!("postmaster@{}", cfg.hostname),
            cfg.hostname.clone(),
            resolver.clone(),
            ctx.outbound_queue.clone(),
            shutdown_rx.clone(),
        );
        eprintln!("DMARC report generation enabled");
    }

    // RBL blocklist monitoring
    if let Some(ref resolver) = ctx.resolver {
        rbl_monitor::start(
            resolver.clone(),
            cfg.hostname.clone(),
            valkey_conn.clone(),
        );
        eprintln!("RBL blocklist monitor started");
    }

    // keep main alive
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for ctrl+c");
    tracing::info!("shutting down");
    let _ = shutdown_tx.send(true);
}

#[allow(clippy::too_many_arguments)]
#[tracing::instrument(
    name = "pop3.conn",
    skip(stream, mailbox_store, users, auth_guard, domain_store, ldap_config, maildir_root),
    fields(peer = %addr),
)]
async fn handle_pop3_connection(
    stream: TcpStream,
    addr: std::net::SocketAddr,
    mailbox_store: Arc<PgMailboxStore>,
    users: Arc<crate::users::UserStore>,
    auth_guard: Arc<AuthGuard>,
    domain_store: Option<Arc<domain_store::DomainStore>>,
    ldap_config: Option<Arc<crate::ldap_auth::LdapConfig>>,
    maildir_root: &str,
) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    let mut session = pop3_session::Pop3Session::new(mailbox_store, users)
        .with_maildir_root(maildir_root)
        .with_auth_guard(auth_guard, addr.ip());
    if let Some(ds) = domain_store {
        session = session.with_domain_store(ds);
    }
    if let Some(ldap) = ldap_config {
        session = session.with_ldap_config(ldap);
    }

    // send greeting
    let greeting = session.greeting();
    if writer.write_all(greeting.as_bytes()).await.is_err() {
        return;
    }

    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) | Err(_) => break, // eof or error
            Ok(_) => {}
        }

        let responses = session.handle_line(&line).await;
        let should_close = session.should_close(&responses);

        for resp in &responses {
            if writer.write_all(resp.as_bytes()).await.is_err() {
                return;
            }
        }
        if writer.flush().await.is_err() {
            return;
        }

        if should_close {
            break;
        }
    }
}

#[tracing::instrument(
    name = "managesieve.conn",
    skip(stream, users, auth_guard, domain_store, ldap_config),
    fields(peer = %addr),
)]
async fn handle_managesieve_connection(
    stream: TcpStream,
    addr: std::net::SocketAddr,
    users: Arc<crate::users::UserStore>,
    auth_guard: Arc<AuthGuard>,
    domain_store: Option<Arc<domain_store::DomainStore>>,
    ldap_config: Option<Arc<crate::ldap_auth::LdapConfig>>,
) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    let mut session = managesieve_session::ManageSieveSession::new(users)
        .with_auth_guard(auth_guard, addr.ip());
    if let Some(ds) = domain_store {
        session = session.with_domain_store(ds);
    }
    if let Some(ldap) = ldap_config {
        session = session.with_ldap_config(ldap);
    }

    // send greeting
    let greeting = session.greeting();
    if writer.write_all(greeting.as_bytes()).await.is_err() {
        return;
    }

    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }

        let responses = session.handle_line(&line).await;
        let should_close = session.should_close(&responses);

        for resp in &responses {
            if writer.write_all(resp.as_bytes()).await.is_err() {
                return;
            }
        }
        if writer.flush().await.is_err() {
            return;
        }

        if should_close {
            break;
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[tracing::instrument(
    name = "imap.conn",
    skip(stream, mailbox_store, users, auth_guard, domain_store, ldap_config, event_bus, hostname, maildir_root),
    fields(peer = %addr),
)]
async fn handle_imap_connection<S>(
    stream: S,
    addr: std::net::SocketAddr,
    mailbox_store: Arc<PgMailboxStore>,
    users: Arc<crate::users::UserStore>,
    auth_guard: Arc<AuthGuard>,
    domain_store: Option<Arc<domain_store::DomainStore>>,
    ldap_config: Option<Arc<crate::ldap_auth::LdapConfig>>,
    event_bus: EventBus,
    hostname: &str,
    maildir_root: &str,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use futures_util::{SinkExt, StreamExt};
    use tokio_util::codec::Framed;

    let mut framed = Framed::new(stream, imap_codec::ImapCodec::new());
    let greeting = imap_session::imap_greeting(hostname);
    if framed.send(greeting).await.is_err() {
        return;
    }

    let mut session = imap_session::ImapSession::new(mailbox_store, users)
        .with_maildir_root(maildir_root)
        .with_auth_guard(auth_guard, addr.ip());
    if let Some(ds) = domain_store {
        session = session.with_domain_store(ds);
    }
    if let Some(ldap) = ldap_config {
        session = session.with_ldap_config(ldap);
    }

    while let Some(result) = framed.next().await {
        match result {
            Ok(imap_codec::ImapInput::Line(line)) => {
                let result = session.handle_line(&line).await;
                match result {
                    imap_session::HandleResult::Responses(responses) => {
                        let is_logout = responses.iter().any(|r| r.windows(3).any(|w| w == b"BYE"));
                        for resp in responses {
                            if framed.send(resp).await.is_err() {
                                return;
                            }
                        }
                        if is_logout {
                            break;
                        }
                    }
                    imap_session::HandleResult::NeedLiteral { continuation, size } => {
                        if framed.send(continuation).await.is_err() {
                            return;
                        }
                        framed.codec_mut().expect_literal(size);
                    }
                    imap_session::HandleResult::EnterIdle { continuation, tag } => {
                        if framed.send(continuation).await.is_err() {
                            return;
                        }

                        let idle_user = session.idle_user().map(|s| s.to_string());
                        let mut rx = event_bus.subscribe();

                        loop {
                            tokio::select! {
                                event = rx.recv() => {
                                    match event {
                                        Ok(event_bus::SmtpEvent::NewMessage { ref user, .. })
                                            if idle_user.as_deref() == Some(user.as_str()) => {
                                                let updates = session.idle_status_update().await;
                                                for u in updates {
                                                    if framed.send(u).await.is_err() {
                                                        return;
                                                    }
                                                }
                                            }
                                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                                        _ => {}
                                    }
                                }
                                frame = framed.next() => {
                                    if let Some(Ok(imap_codec::ImapInput::Line(done_line))) = frame
                                        && done_line.trim().eq_ignore_ascii_case("DONE") {
                                            let resp = mailrs_imap_proto::format_ok(&tag, "IDLE terminated").into_bytes();
                                            if framed.send(resp).await.is_err() {
                                                return;
                                            }
                                        }
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            Ok(imap_codec::ImapInput::LiteralData(data)) => {
                let responses = session.handle_literal_data(&data).await;
                let is_logout = responses.iter().any(|r| r.windows(3).any(|w| w == b"BYE"));
                for resp in responses {
                    if framed.send(resp).await.is_err() {
                        return;
                    }
                }
                if is_logout {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}
