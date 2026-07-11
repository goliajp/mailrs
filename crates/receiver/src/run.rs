//! The standalone receiver process entry point (P6).
//!
//! Assembles a **narrow** `ConnectionContext` — SMTP I/O + anti (greylist /
//! rate / auth via the shared network kevy-server) + antispam pipeline
//! (SPF/DKIM/DMARC + clamav/content) + a spool sink + a cross-process notify
//! publisher. It resolves nothing and stores nothing in spg: recipient
//! resolution / sieve / relay / quota all run later in the core consumer.
//! Accepted mail is written to `{spool_root}/incoming` and a `SpoolDelivered`
//! event is published; the core fetches + delivers it.

use std::sync::Arc;

use mailrs_auth_guard::AuthGuardConfig;
use mailrs_core::event_bus::EventBus;
use mailrs_core::users::UserStore;
use mailrs_shield::greylist::{GreylistConfig, GreylistDb};

use crate::config::ReceiverConfig;
use crate::conn_metrics::ConnectionMetrics;
use crate::inbound::auth_guard::AuthGuardStore;
use crate::inbound::kevy_backends::{
    KevyServerAuthGuardStore, KevyServerGreylistBackend, KevyServerRateLimitStore,
};
use crate::inbound::pipeline::build_inbound_pipeline;
use crate::inbound::rate_limit::{RateLimitStore, TokenBucketConfig};
use crate::inbound::stages::mail_auth::MailAuthResolvers;
use crate::kevy_net::KevyNetClient;
use crate::kevy_notify::{KevyEventPublisher, NOTIFY_CHANNEL, process_origin};
use crate::smtp_session::spool_sink::MaildirSpoolSink;
use crate::smtp_session::{ConnectionContext, handle_plain_connection, handle_tls_connection};

/// Connection / inbound-verdict metrics for the receiver. Emits prometheus
/// counters via the global `metrics` recorder (a no-op if none is installed).
struct ReceiverMetrics;

impl ConnectionMetrics for ReceiverMetrics {
    fn on_connect(&self) {
        metrics::counter!("mailrs_receiver_connections_total").increment(1);
    }
    fn on_disconnect(&self) {
        metrics::counter!("mailrs_receiver_disconnects_total").increment(1);
    }
    fn on_message_delivered(&self) {
        metrics::counter!("mailrs_receiver_spooled_total").increment(1);
    }
    fn inbound_accept(&self) {
        metrics::counter!("mailrs_receiver_inbound_total", "verdict" => "accept").increment(1);
    }
    fn inbound_reject(&self) {
        metrics::counter!("mailrs_receiver_inbound_total", "verdict" => "reject").increment(1);
    }
    fn inbound_defer(&self) {
        metrics::counter!("mailrs_receiver_inbound_total", "verdict" => "defer").increment(1);
    }
    fn inbound_junk(&self) {
        metrics::counter!("mailrs_receiver_inbound_total", "verdict" => "junk").increment(1);
    }
}

/// Run the receiver process: build the narrow context, bind the SMTP
/// listeners, and block until a shutdown signal.
pub async fn run() {
    let cfg = ReceiverConfig::from_env();
    if cfg.kevy_url.is_empty() {
        panic!(
            "MAILRS_KEVY_URL is required for the receiver process — it shares anti state + \
             notify with the core over a network kevy-server"
        );
    }

    tracing::info!(
        hostname = %cfg.hostname,
        spool_root = %cfg.spool_root,
        kevy_url = %cfg.kevy_url,
        "starting mailrs-receiver (split topology)"
    );

    // shared network kevy-server: anti state + the SpoolDelivered notify.
    let kevy_client = Arc::new(KevyNetClient::new(&cfg.kevy_url));

    // anti subsystems, all network-backed (the receiver has no embedded kevy).
    let rate_limiter: Arc<dyn RateLimitStore> = Arc::new(KevyServerRateLimitStore::new(
        kevy_client.clone(),
        TokenBucketConfig {
            capacity: cfg.rate_limit_capacity,
            refill_rate: cfg.rate_limit_refill,
        },
    ));
    let auth_guard: Arc<dyn AuthGuardStore> = Arc::new(KevyServerAuthGuardStore::new(
        kevy_client.clone(),
        AuthGuardConfig::default(),
    ));
    let greylist_db = Some(Arc::new(GreylistDb::with_backend(Arc::new(
        KevyServerGreylistBackend::new(kevy_client.clone()),
    ))));

    // DNS resolver for PTR + SPF/DKIM/DMARC.
    let resolver = hickory_resolver::TokioResolver::builder_tokio()
        .ok()
        .and_then(|mut b| {
            b.options_mut().cache_size = 4096;
            b.build().ok()
        })
        .map(Arc::new);

    let mail_auth_resolvers = if cfg.antispam_enabled {
        resolver.as_ref().map(|r| {
            let dkim = Arc::new(mailrs_dkim::HickoryDkimResolver::new((**r).clone()));
            MailAuthResolvers {
                spf: Arc::new(mailrs_spf::HickoryResolver::new((**r).clone())),
                dkim: dkim.clone(),
                arc: dkim,
                dmarc: r.clone(),
            }
        })
    } else {
        None
    };

    // Whitelist wire-up. Split-topology receiver was shipping with
    // `empty()` here, which meant *every* new sender got greylisted
    // — Gmail / Outlook / etc. all deferred on first attempt even
    // though the whitelist code already knew to skip them. Fix: kick
    // off the same remote-sync task the monolith used, driven by
    // `MAILRS_GREYLIST_WHITELIST_URL` (with a sensible default so
    // operators don't need to set it explicitly).
    let greylist_lists = crate::greylist_sync::empty();
    {
        let url = std::env::var("MAILRS_GREYLIST_WHITELIST_URL").unwrap_or_else(|_| {
            "https://raw.githubusercontent.com/goliajp/mailrs/develop/assets/greylist-whitelist.txt".to_string()
        });
        let interval = std::env::var("MAILRS_GREYLIST_WHITELIST_INTERVAL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3600u64);
        let cache = std::env::var("MAILRS_GREYLIST_CACHE_PATH")
            .unwrap_or_else(|_| "/data/.greylist-whitelist.cache".to_string());
        tracing::info!(%url, interval, %cache, "spawning greylist whitelist sync");
        crate::greylist_sync::spawn_sync_task(greylist_lists.clone(), url, interval, Some(cache));
    }
    // Local admin lists live in the shared network kevy
    // (admin:greylist:local-lists, written by webapi). Reload every 60s
    // so a whitelist entry added in the admin UI takes effect without a
    // receiver restart.
    let greylist_local = crate::greylist_local::empty();
    crate::greylist_local_sync::spawn_reload_task(greylist_local.clone(), kevy_client.clone(), 60);
    let inbound_pipeline = build_inbound_pipeline(
        greylist_db,
        GreylistConfig {
            initial_delay_secs: cfg.greylist_delay_secs,
            ..Default::default()
        },
        greylist_lists,
        greylist_local,
        Some(kevy_client.clone()),
        Some(kevy_client.clone()),
        resolver.clone(),
        mail_auth_resolvers,
        // Record per-message DMARC results into the shared network
        // kevy so admin tooling (and a future aggregate reporter) can
        // consume them. Errors are swallowed inside the sink.
        Some(
            Arc::new(crate::kevy_dmarc::KevyDmarcSink::new(&cfg.kevy_url))
                as Arc<dyn crate::inbound::stages::mail_auth::DmarcReportSink>,
        ),
        cfg.clamav_addr.clone(),
        None, // LLM scoring is the core's post-delivery job
        None, // spam cache is core-side
        cfg.spam_score_threshold,
    );

    // manual TLS only (ACME stays core-side for phase 1); plain SMTP if unset.
    let tls_state = match (&cfg.tls_cert, &cfg.tls_key) {
        (Some(cert), Some(key)) => match mailrs_tls_reload::load_tls_config(cert, key) {
            Ok(c) => Some(mailrs_tls_reload::TlsState::new(
                Arc::try_unwrap(c).unwrap_or_else(|arc| (*arc).clone()),
            )),
            Err(e) => {
                tracing::warn!(error = %e, "failed to load TLS cert/key; starting without TLS");
                None
            }
        },
        _ => None,
    };

    let users = Arc::new(match &cfg.users_file {
        Some(path) => UserStore::load(path).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "failed to load users file; no submission auth");
            UserStore::empty()
        }),
        None => UserStore::empty(),
    });

    // event bus that publishes cross-process events (SpoolDelivered) to the
    // shared kevy-server. The receiver only publishes — the core runs the
    // subscriber bridge.
    let publisher = Arc::new(KevyEventPublisher::new(
        kevy_client.clone(),
        NOTIFY_CHANNEL.to_vec(),
        process_origin(),
    ));
    let event_bus = EventBus::new(1024).with_publisher(publisher);

    let spool_sink: Option<Arc<dyn crate::smtp_session::SpoolSink>> =
        Some(Arc::new(MaildirSpoolSink::new(&cfg.spool_root)));

    // process_tx is unused in spool mode (the DATA handler hands off to the
    // spool sink, never the channel) — a dropped-receiver dummy satisfies the
    // field without spawning a consumer.
    let (process_tx, _dead_rx) = tokio::sync::mpsc::channel(1);

    let ctx = Arc::new(ConnectionContext {
        hostname: cfg.hostname.clone(),
        // unused in spool mode (delivery is core-side); the spool sink owns the path.
        maildir_root: cfg.spool_root.clone(),
        tls_state,
        users,
        event_bus,
        metrics: Arc::new(ReceiverMetrics) as Arc<dyn ConnectionMetrics>,
        rate_limiter,
        local_domains: cfg.local_domains.clone(),
        outbound_enqueue: None,
        resolver,
        dnsbl_zones: cfg.dnsbl_zones.clone(),
        dnsbl_enabled: cfg.dnsbl_enabled,
        antispam_enabled: cfg.antispam_enabled,
        quota_store: None,
        smuggle_protection: cfg.smuggle_protection,
        auth_guard,
        account_store: None,
        queue_notifier: None,
        srs_secret: cfg.srs_secret.clone(),
        ldap_config: None,
        inbound_pipeline,
        spam_lists_client: Some(kevy_client.clone()),
        delivery_executor: mailrs_delivery_executor::DeliveryExecutor::spawn(),
        process_tx,
        spool_sink,
    });

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let tls_available = ctx.tls_state.is_some();

    // SMTP (25) + submission (587) — plain, STARTTLS upgrades if TLS configured.
    let ctx_smtp = ctx.clone();
    crate::listeners::spawn_plain(
        format!("0.0.0.0:{}", cfg.smtp_port),
        "smtp",
        shutdown_rx.clone(),
        move |stream, addr| {
            let ctx = ctx_smtp.clone();
            async move { handle_plain_connection(stream, addr, ctx).await }
        },
    )
    .await;
    let ctx_sub = ctx.clone();
    crate::listeners::spawn_plain(
        format!("0.0.0.0:{}", cfg.submission_port),
        "submission",
        shutdown_rx.clone(),
        move |stream, addr| {
            let ctx = ctx_sub.clone();
            async move { handle_plain_connection(stream, addr, ctx).await }
        },
    )
    .await;
    // SMTPS (465) — implicit TLS, only if a cert is configured.
    if tls_available {
        let ctx_tls = ctx.clone();
        crate::listeners::spawn_plain(
            format!("0.0.0.0:{}", cfg.smtps_port),
            "smtps",
            shutdown_rx.clone(),
            move |stream, addr| {
                let ctx = ctx_tls.clone();
                async move { handle_tls_connection(stream, addr, ctx).await }
            },
        )
        .await;
    }

    tracing::info!("mailrs-receiver listening; awaiting shutdown signal");
    wait_for_shutdown().await;
    let _ = shutdown_tx.send(true);
    tracing::info!("mailrs-receiver shutting down");
}

#[cfg(unix)]
async fn wait_for_shutdown() {
    let mut sigterm =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("sigterm");
    tokio::select! {
        r = tokio::signal::ctrl_c() => { let _ = r; }
        _ = sigterm.recv() => {}
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}
