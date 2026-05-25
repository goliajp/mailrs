#![allow(unused_imports)]
//! Spawn protocol listeners (SMTP / submission / SMTPS / IMAP / IMAPS / POP3 / ManageSieve).

use std::sync::Arc;

use crate::config;
use crate::domain_store;
use crate::event_bus::{EventBus, SmtpEvent};
use crate::inbound::auth_guard::{AuthGuard, AuthGuardConfig};
use crate::web::WebState;
use crate::{
    acme, conversation_cache, dmarc_report, event_bus, health, listeners, oidc_jwt,
    outbound_tls_rpt, rbl_monitor, render_preview, search_index, smtp_session,
    system_config, tls, web, webhook,
};
use mailrs_mailbox::PgMailboxStore;
use crate::imap_session;
use crate::managesieve_session;
use crate::pop3_session;
use crate::smtp_session::ConnectionContext;

/// Spawn the three SMTP-family listeners that all dispatch into
/// the shared `ConnectionContext`:
///   - port `smtp_port` (25/2525) — plain SMTP, STARTTLS optional
///   - port `submission_port` (587/2587) — message submission
///   - port `smtps_port` (465/2465) — implicit-TLS submission
///     (skipped if no TLS configured)
pub(crate) async fn spawn_smtp_listeners(
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
pub(crate) async fn spawn_imap_listeners(
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
    if cfg.disable_plain_imap {
        tracing::info!(
            event = "plaintext_listener_disabled",
            protocol = "imap",
            reason = "MAILRS_DISABLE_PLAIN_IMAP=1 (OWASP A04 — use imaps_port instead)"
        );
    } else if let Some(mb_store) = mailbox_store.as_ref().cloned() {
        tracing::warn!(
            event = "plaintext_listener_active",
            protocol = "imap",
            port = cfg.imap_port,
            "plaintext IMAP transmits credentials in cleartext — set MAILRS_DISABLE_PLAIN_IMAP=1 to use TLS-only imaps_port"
        );
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
pub(crate) async fn spawn_pop3_listener(
    mailbox_store: &Option<Arc<PgMailboxStore>>,
    users: &Arc<crate::users::UserStore>,
    auth_guard: &Arc<AuthGuard>,
    domain_store: &Option<Arc<domain_store::DomainStore>>,
    ldap_config: &Option<Arc<crate::ldap_auth::LdapConfig>>,
    cfg: &config::ServerConfig,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    if cfg.disable_plain_pop3 {
        tracing::info!(
            event = "plaintext_listener_disabled",
            protocol = "pop3",
            reason = "MAILRS_DISABLE_PLAIN_POP3=1 (OWASP A04)"
        );
        return;
    }
    let Some(mb_store) = mailbox_store.as_ref().cloned() else {
        return;
    };
    tracing::warn!(
        event = "plaintext_listener_active",
        protocol = "pop3",
        port = cfg.pop3_port,
        "plaintext POP3 transmits credentials in cleartext — set MAILRS_DISABLE_PLAIN_POP3=1 to disable"
    );
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
pub(crate) async fn spawn_managesieve_listener(
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
