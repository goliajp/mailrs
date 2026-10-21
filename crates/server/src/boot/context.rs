//! The connection context every SMTP session is handed.

use std::sync::Arc;

use crate::inbound::auth_guard::AuthGuardStore;
use crate::web::WebState;
use crate::*;

/// What the context is built from: borrows of values `run_with_roles`
/// keeps using, and the ones it hands over.
pub(super) struct ContextInputs<'a> {
    pub(super) cfg: &'a ServerConfig,
    pub(super) tls_state: &'a Option<crate::tls::TlsState>,
    pub(super) users: &'a Arc<UserStore>,
    pub(super) event_bus: &'a EventBus,
    pub(super) web_state: &'a Arc<WebState>,
    pub(super) rate_limiter: Arc<dyn RateLimitStore>,
    pub(super) resolver: Option<Arc<hickory_resolver::TokioResolver>>,
    pub(super) auth_guard: &'a Arc<dyn AuthGuardStore>,
    pub(super) domain_store: &'a Option<Arc<domain_store::DomainStore>>,
    pub(super) ldap_config: &'a Option<Arc<crate::ldap_auth::LdapConfig>>,
    pub(super) inbound_pipeline: mailrs_inbound::Pipeline,
}

pub(super) fn build_connection_context(inputs: ContextInputs<'_>) -> Arc<ConnectionContext> {
    let ContextInputs {
        cfg,
        tls_state,
        users,
        event_bus,
        web_state,
        rate_limiter,
        resolver,
        auth_guard,
        domain_store,
        ldap_config,
        inbound_pipeline,
    } = inputs;
    Arc::new(ConnectionContext {
        hostname: cfg.hostname.clone(),
        tls_state: tls_state.clone(),
        users: users.clone(),
        event_bus: event_bus.clone(),
        metrics: web_state.clone() as Arc<dyn mailrs_receiver::ConnectionMetrics>,
        rate_limiter,
        runtime: cfg.local_domains.clone().into(),
        org_names: cfg.org_names.clone(),
        org_name_allowed_domains: cfg.org_name_allowed_domains.clone(),
        resolver,
        dnsbl_zones: cfg.dnsbl_zones.clone(),
        dnsbl_enabled: cfg.dnsbl_enabled,
        antispam_enabled: cfg.antispam_enabled,
        smuggle_protection: cfg.smuggle_protection,
        auth_guard: auth_guard.clone(),
        account_store: domain_store.clone().map(|d| {
            Arc::new(crate::account_store::DomainAccounts(d))
                as Arc<dyn mailrs_receiver::AccountStore>
        }),
        credential_store: None,
        recipient_check: None,
        ldap_config: ldap_config.clone(),
        inbound_pipeline,
        // no shared kevy sidecar in the monolith: spam whitelist / blacklist
        // lookups are a fastcore-topology feature, and `None` gives the
        // pipeline empty sets
        spam_lists_client: None,
        // accepted mail goes to the spool; this process's drain delivers it
        spool_sink: Arc::new(
            mailrs_receiver::smtp_session::spool_sink::MaildirSpoolSink::new(
                &super::background::spool_root(cfg),
            ),
        ),
    })
}
