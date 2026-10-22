#![allow(unused_imports)]
//! Spawn protocol listeners (SMTP / submission / SMTPS / IMAP / IMAPS / POP3 / ManageSieve).

use std::sync::Arc;

use crate::config;
use crate::domain_store;
use crate::event_bus::{EventBus, SmtpEvent};
use crate::smtp_session::ConnectionContext;
use crate::web::WebState;
use crate::{
    acme, conversation_cache, dmarc_report, event_bus, health, listeners, oidc_jwt,
    outbound_tls_rpt, rbl_monitor, smtp_session, system_config, tls, web, webhook,
};
use mailrs_mailbox::PgMailboxStore;

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

/// What the IMAP, POP3 and ManageSieve listeners serve from.
pub(crate) struct MailProtocols<'a> {
    pub(crate) mailbox_store: &'a Option<Arc<PgMailboxStore>>,
    pub(crate) users: &'a Arc<crate::users::UserStore>,
    pub(crate) domain_store: &'a Option<Arc<domain_store::DomainStore>>,
    pub(crate) event_bus: &'a EventBus,
    pub(crate) ldap_config: &'a Option<Arc<crate::ldap_auth::LdapConfig>>,
    pub(crate) tls_state: &'a Option<tls::TlsState>,
    pub(crate) cfg: &'a config::ServerConfig,
}

/// IMAP (143), POP3 (110) and ManageSieve (4190), and with a certificate
/// IMAPS (993) and POP3S (995): the listeners and sessions fastcore runs,
/// on the same `MAILRS_*_BIND` variables, over the SQL store.
pub(crate) fn spawn_mail_protocols(p: MailProtocols<'_>) {
    let Some(mailbox) = p.mailbox_store.clone() else {
        tracing::warn!("no mailbox store — IMAP, POP3 and ManageSieve are not served");
        return;
    };
    let dir = crate::protocols::PgDirectory {
        mailbox,
        domain: p.domain_store.clone(),
        users: p.users.clone(),
        ldap: p.ldap_config.clone(),
        message_store: crate::message_store::default_store(),
        events: p.event_bus.clone(),
        maildir_root: p.cfg.maildir_root.clone(),
        hostname: p.cfg.hostname.clone(),
        net_url: std::env::var("MAILRS_KEVY_URL")
            .ok()
            .filter(|s| !s.is_empty()),
    };
    mailrs_mail_protocols::spawn_all(Arc::new(dir), p.tls_state.clone());
}
