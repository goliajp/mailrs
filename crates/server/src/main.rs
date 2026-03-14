mod acme;
mod ai_analyzer;
mod ai_email;
mod ai_spam;
mod api_key_store;
mod codec;
mod config;
mod content_extract;
mod content_worker;
mod dmarc_report;
mod domain_check;
mod domain_store;
pub(crate) mod permission;
mod event_bus;
mod health;
mod html_clean;
mod imap_codec;
mod importance;
mod imap_format;
mod imap_session;
pub mod inbound;
mod inline_image;
mod message_util;
mod structured_data;
mod pg;
mod managesieve_session;
mod pop3_session;
mod ptr_check;
mod sieve;
mod smtp_session;
mod tls;
mod users;
mod valkey_store;
mod mcp;
mod web;
mod webhook;

use std::sync::Arc;

use hickory_resolver::TokioResolver;
use tokio::net::{TcpListener, TcpStream};

use crate::config::{ServerConfig, TlsMode};
use crate::event_bus::{EventBus, SmtpEvent};
use crate::inbound::auth_guard::{AuthGuard, AuthGuardConfig};
use crate::inbound::greylist_db::GreylistDb;
use crate::inbound::greylisting::GreylistConfig;
use crate::inbound::rate_limit::{RateLimiter, TokenBucketConfig};
use crate::smtp_session::ConnectionContext;
use crate::users::UserStore;
use crate::web::WebState;
use mailrs_mailbox::MailboxStore;

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
    if let (Some(ref pg), Some(ref vk)) = (&pg_pool, &valkey_conn) {
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

            acme::spawn_challenge_server(challenge_tokens.clone(), shutdown_rx.clone());

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
                domains.clone(),
                challenge_tokens.clone(),
                cfg.acme_dir.clone(),
                tls.clone(),
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

    let rate_limiter = Arc::new(RateLimiter::new(TokenBucketConfig {
        capacity: cfg.rate_limit_capacity,
        refill_rate: cfg.rate_limit_refill,
    }));

    let outbound_queue = pg_pool.clone();

    // DNS resolver for DNSBL and other lookups
    let resolver = TokioResolver::builder_tokio()
        .ok()
        .map(|b| Arc::new(b.build()));

    // PTR record check
    if let Some(ref r) = resolver {
        ptr_check::check_ptr_record(r, &cfg.hostname).await;
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
        .map(|pool| Arc::new(MailboxStore::new(pool.clone())));

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

    // AI email analyzer (background)
    if cfg.ai_analysis_enabled {
        if let (Some(api_key), Some(ref mb)) = (&cfg.gemini_api_key, &mailbox_store) {
            let gemini_config = ai_email::GeminiConfig::new(api_key.clone());
            ai_analyzer::spawn_analyzer(
                gemini_config,
                mb.clone(),
                event_bus.clone(),
                cfg.maildir_root.clone(),
            );
        } else {
            if cfg.gemini_api_key.is_none() {
                eprintln!("warning: AI analysis enabled but MAILRS_GEMINI_API_KEY not set");
            }
        }
    }

    // content extraction worker (OCR, PDF text)
    if let Some(ref pool) = pg_pool {
        content_worker::spawn_content_worker(pool.clone(), cfg.maildir_root.clone());
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
    if let Some(ref api_key) = cfg.gemini_api_key {
        ws = ws.with_gemini(ai_email::GeminiConfig::new(api_key.clone()));
    }
    if let Some(ref r) = resolver {
        ws = ws.with_resolver(r.clone());
    }
    if let Some(ref sel) = cfg.dkim_selector {
        ws = ws.with_dkim_selector(sel.clone());
    }
    let web_state = Arc::new(ws);

    let users = Arc::new(user_store);

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
        greylist_db,
        greylist_config,
        mail_authenticator,
        spam_score_threshold: cfg.spam_score_threshold,
        mailbox_store: mailbox_store.clone(),
        smuggle_protection: cfg.smuggle_protection,
        auth_guard: auth_guard.clone(),
        domain_store: domain_store.clone(),
        dmarc_report_store: dmarc_report_store.clone(),
        clamav_addr: cfg.clamav_addr.clone(),
        valkey: valkey_conn.clone(),
        ai_config: if cfg.ai_enabled {
            cfg.ai_api_key.as_ref().map(|key| ai_spam::AiSpamConfig {
                api_url: cfg.ai_api_url.clone(),
                api_key: key.clone(),
                model: cfg.ai_model.clone(),
            })
        } else {
            None
        },
        srs_secret: cfg.srs_secret.clone(),
    });

    // port 25/2525: plain SMTP (STARTTLS optional)
    let smtp_addr = format!("0.0.0.0:{}", cfg.smtp_port);
    let smtp_listener = TcpListener::bind(&smtp_addr)
        .await
        .expect("failed to bind SMTP port");
    tracing::info!(addr = smtp_addr.as_str(), "SMTP listening");

    // port 587/2587: submission (STARTTLS optional)
    let sub_addr = format!("0.0.0.0:{}", cfg.submission_port);
    let sub_listener = TcpListener::bind(&sub_addr)
        .await
        .expect("failed to bind submission port");
    tracing::info!(addr = sub_addr.as_str(), "submission listening");

    // spawn SMTP listener
    let ctx_smtp = ctx.clone();
    tokio::spawn(async move {
        loop {
            match smtp_listener.accept().await {
                Ok((stream, addr)) => {
                    let ctx = ctx_smtp.clone();
                    tokio::spawn(async move {
                        smtp_session::handle_plain_connection(stream, addr, ctx).await
                    });
                }
                Err(e) => eprintln!("smtp accept error: {e}"),
            }
        }
    });

    // spawn submission listener
    let ctx_sub = ctx.clone();
    tokio::spawn(async move {
        loop {
            match sub_listener.accept().await {
                Ok((stream, addr)) => {
                    let ctx = ctx_sub.clone();
                    tokio::spawn(async move {
                        smtp_session::handle_plain_connection(stream, addr, ctx).await
                    });
                }
                Err(e) => eprintln!("submission accept error: {e}"),
            }
        }
    });

    // port 465/2465: implicit TLS (only if TLS configured)
    if tls_state.is_some() {
        let smtps_addr = format!("0.0.0.0:{}", cfg.smtps_port);
        let smtps_listener = TcpListener::bind(&smtps_addr)
            .await
            .expect("failed to bind SMTPS port");
        tracing::info!(addr = smtps_addr.as_str(), "SMTPS listening");

        let ctx_tls = ctx.clone();
        tokio::spawn(async move {
            loop {
                match smtps_listener.accept().await {
                    Ok((stream, addr)) => {
                        let ctx = ctx_tls.clone();
                        tokio::spawn(async move {
                            smtp_session::handle_tls_connection(stream, addr, ctx).await
                        });
                    }
                    Err(e) => eprintln!("smtps accept error: {e}"),
                }
            }
        });
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

    // IMAP listener
    if let Some(ref mb_store) = mailbox_store {
        let imap_addr = format!("0.0.0.0:{}", cfg.imap_port);
        let imap_listener = TcpListener::bind(&imap_addr)
            .await
            .expect("failed to bind IMAP port");
        tracing::info!(addr = imap_addr.as_str(), "IMAP listening");

        let imap_mb_store = mb_store.clone();
        let imap_users = users.clone();
        let imap_hostname = cfg.hostname.clone();
        let imap_maildir_root = cfg.maildir_root.clone();
        let imap_auth_guard = auth_guard.clone();
        let imap_domain_store = domain_store.clone();
        let imap_event_bus = event_bus.clone();
        tokio::spawn(async move {
            loop {
                match imap_listener.accept().await {
                    Ok((stream, addr)) => {
                        let mb = imap_mb_store.clone();
                        let u = imap_users.clone();
                        let h = imap_hostname.clone();
                        let mr = imap_maildir_root.clone();
                        let ag = imap_auth_guard.clone();
                        let ds = imap_domain_store.clone();
                        let eb = imap_event_bus.clone();
                        tokio::spawn(async move {
                            handle_imap_connection(stream, addr, mb, u, ag, ds, eb, &h, &mr).await;
                        });
                    }
                    Err(e) => eprintln!("imap accept error: {e}"),
                }
            }
        });
    }

    // IMAPS listener (implicit TLS, port 993)
    if tls_state.is_some() {
        if let Some(ref mb_store) = mailbox_store {
            let imaps_addr = format!("0.0.0.0:{}", cfg.imaps_port);
            let imaps_listener = TcpListener::bind(&imaps_addr)
                .await
                .expect("failed to bind IMAPS port");
            tracing::info!(addr = imaps_addr.as_str(), "IMAPS listening");

            // safe: outer `if tls_state.is_some()` guarantees this
            let Some(imaps_tls) = tls_state.clone() else {
                unreachable!("tls_state checked above");
            };
            let imaps_mb_store = mb_store.clone();
            let imaps_users = users.clone();
            let imaps_hostname = cfg.hostname.clone();
            let imaps_maildir_root = cfg.maildir_root.clone();
            let imaps_auth_guard = auth_guard.clone();
            let imaps_domain_store = domain_store.clone();
            let imaps_event_bus = event_bus.clone();
            tokio::spawn(async move {
                loop {
                    match imaps_listener.accept().await {
                        Ok((stream, addr)) => {
                            let tls = imaps_tls.clone();
                            let mb = imaps_mb_store.clone();
                            let u = imaps_users.clone();
                            let h = imaps_hostname.clone();
                            let mr = imaps_maildir_root.clone();
                            let ag = imaps_auth_guard.clone();
                            let ds = imaps_domain_store.clone();
                            let eb = imaps_event_bus.clone();
                            tokio::spawn(async move {
                                match tls.acceptor().accept(stream).await {
                                    Ok(tls_stream) => {
                                        handle_imap_connection(
                                            tls_stream, addr, mb, u, ag, ds, eb, &h, &mr,
                                        )
                                        .await;
                                    }
                                    Err(e) => {
                                        eprintln!("imaps tls handshake error from {addr}: {e}");
                                    }
                                }
                            });
                        }
                        Err(e) => eprintln!("imaps accept error: {e}"),
                    }
                }
            });
        }
    }

    // POP3 listener
    if let Some(ref mb_store) = mailbox_store {
        let pop3_addr = format!("0.0.0.0:{}", cfg.pop3_port);
        let pop3_listener = TcpListener::bind(&pop3_addr)
            .await
            .expect("failed to bind POP3 port");
        tracing::info!(addr = pop3_addr.as_str(), "POP3 listening");

        let pop3_mb_store = mb_store.clone();
        let pop3_users = users.clone();
        let pop3_maildir_root = cfg.maildir_root.clone();
        let pop3_auth_guard = auth_guard.clone();
        let pop3_domain_store = domain_store.clone();
        tokio::spawn(async move {
            loop {
                match pop3_listener.accept().await {
                    Ok((stream, addr)) => {
                        let mb = pop3_mb_store.clone();
                        let u = pop3_users.clone();
                        let mr = pop3_maildir_root.clone();
                        let ag = pop3_auth_guard.clone();
                        let ds = pop3_domain_store.clone();
                        tokio::spawn(async move {
                            handle_pop3_connection(stream, addr, mb, u, ag, ds, &mr).await;
                        });
                    }
                    Err(e) => eprintln!("pop3 accept error: {e}"),
                }
            }
        });
    }

    // ManageSieve listener (RFC 5804)
    {
        let sieve_addr = format!("0.0.0.0:{}", cfg.managesieve_port);
        let sieve_listener = TcpListener::bind(&sieve_addr)
            .await
            .expect("failed to bind ManageSieve port");
        tracing::info!(addr = sieve_addr.as_str(), "ManageSieve listening");

        let sieve_users = users.clone();
        let sieve_auth_guard = auth_guard.clone();
        let sieve_domain_store = domain_store.clone();
        tokio::spawn(async move {
            loop {
                match sieve_listener.accept().await {
                    Ok((stream, addr)) => {
                        let u = sieve_users.clone();
                        let ag = sieve_auth_guard.clone();
                        let ds = sieve_domain_store.clone();
                        tokio::spawn(async move {
                            handle_managesieve_connection(stream, addr, u, ag, ds).await;
                        });
                    }
                    Err(e) => eprintln!("managesieve accept error: {e}"),
                }
            }
        });
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

            // bridge outbound events to event bus
            let eb = event_bus.clone();
            worker = worker.with_event_sender(Arc::new(move |evt| {
                use mailrs_outbound_queue::DeliveryEvent;
                let smtp_evt = match evt {
                    DeliveryEvent::Attempt { queue_id, domain } => {
                        SmtpEvent::DeliveryAttempt { queue_id, domain }
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

            let rx = shutdown_rx.clone();
            tokio::spawn(async move {
                worker.run(rx).await;
            });
            tracing::info!("delivery worker started");
        } else {
            eprintln!("warning: queue_db configured but no DNS resolver available, delivery worker disabled");
        }
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
    if let (Some(ref dmarc_store), Some(ref resolver)) = (&dmarc_report_store, &ctx.resolver) {
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

    // keep main alive
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for ctrl+c");
    tracing::info!("shutting down");
    let _ = shutdown_tx.send(true);
}

async fn handle_pop3_connection(
    stream: TcpStream,
    addr: std::net::SocketAddr,
    mailbox_store: Arc<MailboxStore>,
    users: Arc<crate::users::UserStore>,
    auth_guard: Arc<AuthGuard>,
    domain_store: Option<Arc<domain_store::DomainStore>>,
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

async fn handle_managesieve_connection(
    stream: TcpStream,
    addr: std::net::SocketAddr,
    users: Arc<crate::users::UserStore>,
    auth_guard: Arc<AuthGuard>,
    domain_store: Option<Arc<domain_store::DomainStore>>,
) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    let mut session = managesieve_session::ManageSieveSession::new(users)
        .with_auth_guard(auth_guard, addr.ip());
    if let Some(ds) = domain_store {
        session = session.with_domain_store(ds);
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

async fn handle_imap_connection<S>(
    stream: S,
    addr: std::net::SocketAddr,
    mailbox_store: Arc<MailboxStore>,
    users: Arc<crate::users::UserStore>,
    auth_guard: Arc<AuthGuard>,
    domain_store: Option<Arc<domain_store::DomainStore>>,
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
                                        Ok(event_bus::SmtpEvent::NewMessage { ref user, .. }) => {
                                            if idle_user.as_deref() == Some(user.as_str()) {
                                                let updates = session.idle_status_update().await;
                                                for u in updates {
                                                    if framed.send(u).await.is_err() {
                                                        return;
                                                    }
                                                }
                                            }
                                        }
                                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                                        _ => {}
                                    }
                                }
                                frame = framed.next() => {
                                    match frame {
                                        Some(Ok(imap_codec::ImapInput::Line(done_line))) => {
                                            if done_line.trim().eq_ignore_ascii_case("DONE") {
                                                let resp = mailrs_imap_proto::format_ok(&tag, "IDLE terminated").into_bytes();
                                                if framed.send(resp).await.is_err() {
                                                    return;
                                                }
                                            }
                                        }
                                        _ => {}
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
