#![allow(unused_imports)]
//! Misc runtime tasks: web server, webhooks, DMARC aggregate, RBL self-monitor.

use std::sync::Arc;

use crate::config;
use crate::domain_store;
use crate::event_bus::{EventBus, SmtpEvent};
use crate::inbound::auth_guard::{AuthGuard, AuthGuardConfig};
use crate::web::WebState;
use crate::{
    acme, conversation_cache, dmarc_report, event_bus, health, listeners, oidc_jwt,
    outbound_tls_rpt, rbl_monitor, render_preview, search_index, smtp_session, system_config, tls,
    web, webhook,
};
use mailrs_mailbox::PgMailboxStore;
use tokio::net::TcpListener;

/// Bind the web HTTP listener, spawn the session-cleanup task,
/// spawn the domain-store cache-eviction task (60s interval), and
/// spawn the axum serve task with graceful shutdown wired to the
/// shared `shutdown_rx`.
pub(crate) async fn spawn_web_server(
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
                    tracing::debug!(event = "domain_cache_evict", count = evicted);
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
pub(crate) fn spawn_webhook_subsystem(
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
        tracing::info!(event = "subsystem_started", subsystem = "webhook_global");
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
        tracing::info!(event = "subsystem_started", subsystem = "webhook");
    }
}

/// Spawn the daily DMARC aggregate-report builder + submitter.
/// Reads per-message DMARC outcomes the inbound pipeline
/// recorded, batches per-domain rua reports, sends via the
/// outbound queue addressed to `postmaster@<hostname>`.
/// No-op without a DMARC report store or DNS resolver.
pub(crate) fn spawn_dmarc_aggregate_task(
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
    tracing::info!(event = "subsystem_started", subsystem = "dmarc_report");
}

/// Spawn the RBL (DNS blocklist) self-monitor — periodically
/// checks whether our hostname is listed on common RBLs and logs
/// a warning if so. Helps operators notice reputation hits before
/// outbound delivery starts bouncing.
pub(crate) fn spawn_rbl_monitor(
    resolver: &Option<Arc<hickory_resolver::TokioResolver>>,
    hostname: &str,
    valkey_conn: &Option<redis::aio::ConnectionManager>,
) {
    let Some(resolver) = resolver else { return };
    rbl_monitor::start(resolver.clone(), hostname.to_string(), valkey_conn.clone());
    tracing::info!(event = "subsystem_started", subsystem = "rbl_monitor");
}
