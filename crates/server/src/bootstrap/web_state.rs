#![allow(unused_imports)]
//! `WebState` construction from optional backends + OIDC env reader.

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

/// Inputs to [`build_web_state`] — bundling them in a struct
/// avoids a 14-argument fn signature and makes the call site
/// readable. All fields are borrows of values already live in
/// `main` at the build point.
pub(crate) struct WebStateInputs<'a> {
    pub(crate) cfg: &'a config::ServerConfig,
    pub(crate) event_bus: EventBus,
    pub(crate) auth_guard: Arc<AuthGuard>,
    pub(crate) health_state: health::HealthState,
    pub(crate) pg_pool: &'a Option<sqlx::PgPool>,
    pub(crate) valkey_conn: &'a Option<redis::aio::ConnectionManager>,
    pub(crate) outbound_queue: &'a Option<sqlx::PgPool>,
    pub(crate) mailbox_store: &'a Option<Arc<PgMailboxStore>>,
    pub(crate) domain_store: &'a Option<Arc<domain_store::DomainStore>>,
    pub(crate) llm_provider: &'a Option<Arc<dyn mailrs_intelligence::provider::LlmProvider>>,
    pub(crate) resolver: &'a Option<Arc<hickory_resolver::TokioResolver>>,
    pub(crate) ldap_config: &'a Option<Arc<crate::ldap_auth::LdapConfig>>,
    pub(crate) meili_client: Option<&'a Arc<search_index::MeiliClient>>,
    pub(crate) system_config_store: Arc<system_config::SystemConfigStore>,
}

/// Build the `WebState` from optional backends. Optional pieces
/// (PG, Valkey, mailbox, domain store, LLM, resolver, LDAP,
/// Meilisearch, Chrome render preview, OIDC client) only attach
/// when the corresponding backend was successfully initialized.
pub(crate) fn build_web_state(i: WebStateInputs<'_>) -> WebState {
    let smtp_snapshot = crate::web::SmtpConfigSnapshot {
        hostname: i.cfg.hostname.clone(),
        smtp_port: i.cfg.smtp_port,
        submission_port: i.cfg.submission_port,
        imap_port: i.cfg.imap_port,
        local_domains: i.cfg.local_domains.clone(),
        max_message_size: None,
        tls_enabled: i.cfg.has_tls() || i.cfg.acme_email.is_some(),
    };

    let mut ws = WebState::new(i.event_bus)
        .with_maildir_root(i.cfg.maildir_root.clone())
        .with_hostname(i.cfg.hostname.clone())
        .with_auth_guard(i.auth_guard)
        .with_health(i.health_state)
        .with_smtp_config(smtp_snapshot)
        .with_system_config(i.system_config_store);

    if let Some(pool) = i.pg_pool {
        ws = ws.with_pg(pool.clone());
    }
    if let Some(vk) = i.valkey_conn {
        ws = ws.with_valkey(vk.clone());
    }
    if let Some(q) = i.outbound_queue {
        ws = ws.with_queue(q.clone());
    }
    if let Some(mb) = i.mailbox_store {
        ws = ws.with_mailbox(mb.clone());
    }
    if let Some(ds) = i.domain_store {
        ws = ws.with_domain_store(ds.clone());
    }
    if let Some(ref mode) = i.cfg.mta_sts_mode {
        ws = ws.with_mta_sts(
            mode.clone(),
            i.cfg.mta_sts_mx.clone(),
            i.cfg.mta_sts_max_age,
            i.cfg.mta_sts_id.clone(),
        );
    }
    if let Some(provider) = i.llm_provider {
        ws = ws.with_llm(provider.clone());
    }
    if let Some(r) = i.resolver {
        ws = ws.with_resolver(r.clone());
    }
    if let Some(ref sel) = i.cfg.dkim_selector {
        ws = ws.with_dkim_selector(sel.clone());
    }
    if let Some(ldap) = i.ldap_config {
        ws = ws.with_ldap_config(ldap.clone());
    }
    if let Some(ref url) = i.cfg.chrome_cdp_url {
        let client = Arc::new(render_preview::RenderPreviewClient::new(url.clone(), 5));
        ws = ws.with_render_preview(client);
        eprintln!("Email render preview enabled (Chrome CDP: {url})");
    }
    if let Some(meili) = i.meili_client {
        ws = ws.with_meili(meili.clone());
    }
    if let Some(oidc) = oidc_client_from_env(&i.cfg.hostname) {
        tracing::info!("OIDC client configured (issuer={})", oidc.token_url);
        ws = ws.with_oidc(oidc);
    }
    ws
}


/// Read `MAILRS_OIDC_*` env vars and build the external-IdP
/// "Sign in with X" config. Returns None unless the three
/// required vars (CLIENT_ID, CLIENT_SECRET, ISSUER) are all set;
/// derives optional URLs from `ISSUER` if not overridden.
pub(crate) fn oidc_client_from_env(hostname: &str) -> Option<crate::web::OidcConfig> {
    let client_id = std::env::var("MAILRS_OIDC_CLIENT_ID").ok()?;
    let client_secret = std::env::var("MAILRS_OIDC_CLIENT_SECRET").ok()?;
    let issuer = std::env::var("MAILRS_OIDC_ISSUER").ok()?;
    let redirect_uri = std::env::var("MAILRS_OIDC_REDIRECT_URI")
        .unwrap_or_else(|_| format!("https://{hostname}/api/auth/oidc/callback"));
    let authorize_url = std::env::var("MAILRS_OIDC_AUTHORIZE_URL")
        .unwrap_or_else(|_| format!("{issuer}/authorize"));
    let token_url =
        std::env::var("MAILRS_OIDC_TOKEN_URL").unwrap_or_else(|_| format!("{issuer}/token"));
    let userinfo_url = std::env::var("MAILRS_OIDC_USERINFO_URL")
        .unwrap_or_else(|_| format!("{issuer}/userinfo"));
    Some(crate::web::OidcConfig {
        client_id,
        client_secret,
        authorize_url,
        token_url,
        userinfo_url,
        redirect_uri,
    })
}
