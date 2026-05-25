#![allow(unused_imports)]
//! Brute-force `AuthGuard` construction + periodic cleanup task.

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

/// Construct the brute-force `AuthGuard` from per-account + per-IP
/// thresholds in `cfg`, and spawn a 5-minute periodic cleanup task
/// that evicts entries past their lockout window.
pub(crate) fn init_auth_guard(cfg: &config::ServerConfig) -> Arc<AuthGuard> {
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
