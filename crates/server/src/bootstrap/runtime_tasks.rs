//! The webhook background tasks.

use crate::config;
use crate::event_bus::EventBus;
use crate::webhook;

/// The global webhook: a fire-and-forget POST on every arriving message, to
/// `MAILRS_WEBHOOK_URL`. Not started when that is unset.
pub(crate) fn spawn_global_webhook(
    event_bus: &EventBus,
    cfg: &config::ServerConfig,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    let Some(url) = cfg.webhook_url.clone().filter(|u| !u.is_empty()) else {
        return;
    };
    let api_key = cfg.webhook_api_key.clone();
    let eb = event_bus.clone();
    tokio::spawn(async move {
        webhook::run(&eb, url, api_key, shutdown_rx).await;
    });
    tracing::info!(event = "subsystem_started", subsystem = "webhook_global");
}
