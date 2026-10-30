//! Spawn the IMAP / IMAPS / POP3 / POP3S / ManageSieve listeners.

use std::sync::Arc;

use crate::config;
use crate::domain_store;
use crate::event_bus::EventBus;
use crate::tls;
use mailrs_mailbox::PgMailboxStore;

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
    let guard = mailrs_auth_guard::kevy::guard_for(dir.net_url.as_deref());
    let url = dir.net_url.clone();
    let guarded = mailrs_mail_protocols::Guarded::new(Arc::new(dir), guard);
    if let Some(url) = url {
        let signins = guarded.signed_in();
        mailrs_core_sidestate::keys::signout::listen(url, move |account| {
            signins.end(account);
        });
    }
    mailrs_mail_protocols::spawn_all(Arc::new(guarded), p.tls_state.clone());
}
