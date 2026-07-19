#![allow(unused_imports)]
//! Spawn the outbound delivery worker + TLSRPT 24h flush.

use std::sync::Arc;

use crate::config;
use crate::domain_store;
use crate::event_bus::{EventBus, SmtpEvent};
use crate::inbound::auth_guard::{AuthGuard, AuthGuardConfig};
use crate::web::WebState;
use crate::{
    acme, conversation_cache, dmarc_report, event_bus, health, listeners, oidc_jwt,
    outbound_tls_rpt, rbl_monitor, smtp_session, system_config, tls, web, webhook,
};
use mailrs_mailbox::PgMailboxStore;

/// Spawn the outbound `DeliveryWorker` and its 24h TLSRPT
/// flush companion task. Configures DKIM signing if the cfg
/// has selector/domain/key set; bridges DeliveryEvent into the
/// SmtpEvent bus; and persists per-attempt TLS outcomes to a
/// PG-backed TLSRPT store so the daily flush survives restart.
///
/// No-op if `outbound_queue` is None (PG unavailable) or
/// `resolver` is None (DNS unavailable) — in either case
/// delivery would fail anyway.
pub(crate) fn spawn_outbound_delivery(
    outbound_queue: Option<&crate::pg::BackendPool>,
    resolver: Option<&Arc<hickory_resolver::TokioResolver>>,
    kevy: Option<&crate::kevy_store::KevyStore>,
    cfg: &config::ServerConfig,
    event_bus: EventBus,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    let Some(pool) = outbound_queue else { return };
    let Some(resolver) = resolver else {
        tracing::warn!(
            event = "subsystem_disabled",
            subsystem = "delivery_worker",
            reason = "no DNS resolver",
            "queue_db configured but no resolver — outbound delivery off"
        );
        return;
    };

    let mut worker = build_delivery_worker(pool, resolver, kevy, cfg);
    let tls_rpt_obs = Arc::new(outbound_tls_rpt::TlsRptObserver::new(
        outbound_tls_rpt::PgTlsRptStore::new(pool.clone()).into_arc(),
    ));
    worker = worker.with_event_sender(make_delivery_event_sender(event_bus, tls_rpt_obs.clone()));

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

/// Construct the outbound `DeliveryWorker` with the shared in-process
/// kevy [`Store`] (for `queue:notify` wakeup) and DKIM signing key (if
/// configured). Pure construction — no spawning.
pub(crate) fn build_delivery_worker(
    pool: &crate::pg::BackendPool,
    resolver: &Arc<hickory_resolver::TokioResolver>,
    kevy: Option<&crate::kevy_store::KevyStore>,
    cfg: &config::ServerConfig,
) -> mailrs_outbound_queue::DeliveryWorker {
    let mut worker = mailrs_outbound_queue::DeliveryWorker::new(
        mailrs_outbound_queue::worker::WorkerConfig::default(),
        pool.clone(),
        (**resolver).clone(),
        cfg.hostname.clone(),
    );
    if let Some(store) = kevy {
        worker = worker.with_kevy(store.as_ref().clone());
    }
    if let (Some(selector), Some(domain), Some(key_path)) = (
        &cfg.dkim_selector,
        &cfg.dkim_domain,
        &cfg.dkim_private_key_path,
    ) {
        match std::fs::read_to_string(key_path) {
            Ok(pem) => {
                // Build the per-domain map from MAILRS_DKIM_KEYS. Each
                // entry overrides the default `(selector, domain, key)`
                // tuple when the outbound message's From: domain matches.
                let mut extra_keys = std::collections::HashMap::new();
                for (extra_domain, extra_selector, extra_path) in &cfg.dkim_extra_keys {
                    match std::fs::read_to_string(extra_path) {
                        Ok(extra_pem) => {
                            extra_keys.insert(
                                extra_domain.clone(),
                                mailrs_outbound_queue::DkimDomainKey {
                                    selector: extra_selector.clone(),
                                    private_key_pem: extra_pem,
                                    ..Default::default()
                                },
                            );
                            tracing::info!(
                                event = "subsystem_extra_key",
                                subsystem = "dkim_signing",
                                domain = %extra_domain,
                                selector = %extra_selector,
                                path = %extra_path.display()
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                event = "subsystem_extra_key_failed",
                                subsystem = "dkim_signing",
                                domain = %extra_domain,
                                path = %extra_path.display(),
                                error = %e,
                                "extra DKIM key unreadable — entry skipped"
                            );
                        }
                    }
                }
                worker = worker.with_dkim(mailrs_outbound_queue::DkimSignConfig {
                    selector: selector.clone(),
                    domain: domain.clone(),
                    private_key_pem: pem,
                    extra_keys,
                    ..Default::default()
                });
                tracing::info!(
                    event = "subsystem_started",
                    subsystem = "dkim_signing",
                    selector = %selector,
                    domain = %domain,
                    extra_domain_count = cfg.dkim_extra_keys.len(),
                );
            }
            Err(e) => {
                tracing::warn!(
                    event = "subsystem_disabled",
                    subsystem = "dkim_signing",
                    path = %key_path.display(),
                    error = %e,
                    "DKIM private key unreadable"
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
pub(crate) fn make_delivery_event_sender(
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
                    tls_obs
                        .record_tls_attempt(&domain, &mx_host, &outcome)
                        .await;
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
pub(crate) fn spawn_tlsrpt_flush_task(
    tls_rpt_obs: Arc<outbound_tls_rpt::TlsRptObserver>,
    hostname: String,
    resolver: Arc<hickory_resolver::TokioResolver>,
    pool: crate::pg::BackendPool,
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
