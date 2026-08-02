use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use arc_swap::ArcSwapOption;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};

use crate::domain_store::DomainStore;
use crate::event_bus::EventBus;
use crate::health::HealthState;
use crate::inbound::auth_guard::AuthGuardStore;
use mailrs_mailbox::PgMailboxStore;

mod limits;
mod session;

pub(crate) use limits::*;
pub(crate) use session::*;

mod admin;

mod ai_assist;

mod api_key;

mod auth;

mod autodiscover;

mod calendar_api;

mod classify;

mod conversations;

mod dav;

mod jmap;

pub(crate) mod mail;

mod oidc_provider;

pub(crate) mod rate_limit;

mod request_id;

mod router;

mod rsvp;

mod system_config;

mod templates;

mod webhook;

mod ws;

pub(crate) use auth::{AuthMethod, AuthUser};
pub(crate) use classify::classify_email;
pub use router::router;

/// non-sensitive SMTP configuration snapshot exposed via the admin API
#[derive(Clone, serde::Serialize)]
pub struct SmtpConfigSnapshot {
    pub hostname: String,
    pub smtp_port: u16,
    pub submission_port: u16,
    pub imap_port: u16,
    pub local_domains: Vec<String>,
    pub max_message_size: Option<u64>,
    pub tls_enabled: bool,
}

/// OIDC client configuration for "Sign in with GOLIA" (or any external IdP)
#[derive(Clone)]
pub struct OidcConfig {
    pub client_id: String,
    pub client_secret: String,
    pub authorize_url: String,
    pub token_url: String,
    pub userinfo_url: String,
    pub redirect_uri: String,
}

pub struct WebState {
    pub event_bus: EventBus,
    pub started_at: Instant,
    pub total_connections: AtomicU64,
    pub total_messages: AtomicU64,
    pub active_connections: AtomicU64,
    /// Per-verdict counters for inbound DATA decisions. Incremented in
    /// the SMTP DATA handler after `mailrs_inbound::Pipeline::run`
    /// returns. Exposed via the Prometheus `/metrics` endpoint as
    /// `mailrs_inbound_verdict_total{verdict="…"}` so operators can
    /// see the rejection mix at a glance.
    pub inbound_accept_total: AtomicU64,
    pub inbound_reject_total: AtomicU64,
    pub inbound_defer_total: AtomicU64,
    pub inbound_junk_total: AtomicU64,
    /// Per-outcome counters for web login attempts. A sustained spike
    /// in `auth_failure_total` against a single account or from a
    /// single IP is the canonical password-attack signal. Exposed as
    /// `mailrs_auth_total{outcome="success|failure"}`.
    pub auth_success_total: AtomicU64,
    pub auth_failure_total: AtomicU64,
    pub outbound_queue: Option<crate::pg::BackendPool>,
    pub mailbox_store: Option<Arc<PgMailboxStore>>,
    pub domain_store: Option<Arc<DomainStore>>,
    /// Backend-agnostic alias table — Step 3 of RFC 20260705.
    /// `Some` = shared kevy (network backend) is source of truth,
    /// `None` = legacy PG-backed DomainStore.aliases path. Populated
    /// from `MAILRS_ALIAS_STORE_BACKEND=network` at boot; unset keeps
    /// current behaviour so existing deploys are undisturbed.
    pub alias_store: Option<Arc<dyn mailrs_alias_store::AliasStore>>,
    /// Swappable delivered-message backend (maildir today). Local web
    /// delivery (INBOX + Sent copy) writes through this seam; see
    /// [`crate::message_store`].
    pub message_store: Arc<dyn crate::message_store::MessageStore>,
    pub maildir_root: String,
    pub hostname: String,
    pub(crate) sessions: SessionStore,
    pub auth_guard: Option<Arc<dyn AuthGuardStore>>,
    pub mta_sts_mode: Option<String>,
    pub mta_sts_mx: Vec<String>,
    pub mta_sts_max_age: u64,
    pub mta_sts_id: String,
    pub health: Option<HealthState>,
    pub pg_pool: Option<crate::pg::BackendPool>,
    /// In-process embed kevy store — the only kevy handle in WebState.
    pub kevy_embed: Option<crate::kevy_store::KevyStore>,
    pub llm_config: Option<Arc<dyn mailrs_intelligence::provider::LlmProvider>>,
    pub resolver: Option<Arc<hickory_resolver::TokioResolver>>,
    pub dkim_selector: Option<String>,
    pub smtp_config: Option<SmtpConfigSnapshot>,
    pub web_rate_limiter: Arc<rate_limit::WebRateLimiter>,
    pub ldap_config: Option<Arc<crate::ldap_auth::LdapConfig>>,
    pub oidc_config: Option<OidcConfig>,
    pub system_config: Option<Arc<crate::system_config::SystemConfigStore>>,
    /// Prometheus exporter handle for `/metrics` rendering. `None`
    /// only in unit tests that don't install the global recorder.
    pub metrics_handle: Option<metrics_exporter_prometheus::PrometheusHandle>,
    /// Phase 2 greylist local lists snapshot — shared with the inbound
    /// pipeline. Admin endpoints reload this after each write.
    pub greylist_local: crate::greylist_local::GreylistLocalHandle,
}

impl WebState {
    pub fn new(event_bus: EventBus) -> Self {
        Self {
            event_bus,
            started_at: Instant::now(),
            total_connections: AtomicU64::new(0),
            total_messages: AtomicU64::new(0),
            active_connections: AtomicU64::new(0),
            inbound_accept_total: AtomicU64::new(0),
            inbound_reject_total: AtomicU64::new(0),
            inbound_defer_total: AtomicU64::new(0),
            inbound_junk_total: AtomicU64::new(0),
            auth_success_total: AtomicU64::new(0),
            auth_failure_total: AtomicU64::new(0),
            outbound_queue: None,
            mailbox_store: None,
            domain_store: None,
            alias_store: None,
            message_store: crate::message_store::default_store(),
            maildir_root: String::new(),
            hostname: String::new(),
            sessions: SessionStore::new(),
            auth_guard: None,
            mta_sts_mode: None,
            mta_sts_mx: vec![],
            mta_sts_max_age: 604800,
            mta_sts_id: String::new(),
            health: None,
            pg_pool: None,
            kevy_embed: None,
            llm_config: None,
            resolver: None,
            dkim_selector: None,
            smtp_config: None,
            web_rate_limiter: Arc::new(rate_limit::WebRateLimiter::new()),
            ldap_config: None,
            oidc_config: None,
            system_config: None,
            metrics_handle: None,
            greylist_local: crate::greylist_local::empty(),
        }
    }

    pub fn with_metrics_handle(mut self, h: metrics_exporter_prometheus::PrometheusHandle) -> Self {
        self.metrics_handle = Some(h);
        self
    }

    pub fn with_smtp_config(mut self, snapshot: SmtpConfigSnapshot) -> Self {
        self.smtp_config = Some(snapshot);
        self
    }

    pub fn with_llm(
        mut self,
        provider: Arc<dyn mailrs_intelligence::provider::LlmProvider>,
    ) -> Self {
        self.llm_config = Some(provider);
        self
    }

    pub fn with_resolver(mut self, resolver: Arc<hickory_resolver::TokioResolver>) -> Self {
        self.resolver = Some(resolver);
        self
    }

    pub fn with_dkim_selector(mut self, selector: String) -> Self {
        self.dkim_selector = Some(selector);
        self
    }

    pub fn with_queue(mut self, pool: crate::pg::BackendPool) -> Self {
        self.outbound_queue = Some(pool);
        self
    }

    pub fn with_mailbox(mut self, store: Arc<PgMailboxStore>) -> Self {
        self.mailbox_store = Some(store);
        self
    }

    pub fn with_domain_store(mut self, store: Arc<DomainStore>) -> Self {
        self.domain_store = Some(store);
        self
    }

    /// Attach a backend-agnostic AliasStore (Step 3 of RFC 20260705).
    /// When present, monolith admin handlers + inbound alias resolve go
    /// through this trait — a `NetworkKevyAliasStore` here makes pg-core
    /// mode read the same shared kevy that fastcore mode reads, so the
    /// v2 dual-mode switch preserves alias data across cutover.
    ///
    /// When None, existing PG-backed DomainStore.aliases path stays.
    pub fn with_alias_store(mut self, store: Arc<dyn mailrs_alias_store::AliasStore>) -> Self {
        self.alias_store = Some(store);
        self
    }

    pub fn with_mta_sts(mut self, mode: String, mx: Vec<String>, max_age: u64, id: String) -> Self {
        self.mta_sts_mode = Some(mode);
        self.mta_sts_mx = mx;
        self.mta_sts_max_age = max_age;
        self.mta_sts_id = id;
        self
    }

    pub fn with_auth_guard(mut self, guard: Arc<dyn AuthGuardStore>) -> Self {
        self.auth_guard = Some(guard);
        self
    }

    pub fn with_maildir_root(mut self, root: String) -> Self {
        self.maildir_root = root;
        self
    }

    pub fn with_ldap_config(mut self, config: Arc<crate::ldap_auth::LdapConfig>) -> Self {
        self.ldap_config = Some(config);
        self
    }

    pub fn with_oidc(mut self, config: OidcConfig) -> Self {
        self.oidc_config = Some(config);
        self
    }

    pub fn with_system_config(
        mut self,
        store: Arc<crate::system_config::SystemConfigStore>,
    ) -> Self {
        self.system_config = Some(store);
        self
    }

    pub fn with_hostname(mut self, hostname: String) -> Self {
        self.hostname = hostname;
        self
    }

    pub fn with_health(mut self, health: HealthState) -> Self {
        self.health = Some(health);
        self
    }

    pub fn with_pg(mut self, pool: crate::pg::BackendPool) -> Self {
        self.pg_pool = Some(pool);
        self
    }

    pub fn on_connect(&self) {
        self.total_connections.fetch_add(1, Ordering::Relaxed);
        self.active_connections.fetch_add(1, Ordering::Relaxed);
        metrics::counter!("mailrs_connections_total").increment(1);
        metrics::gauge!("mailrs_connections_active").increment(1.0);
    }

    pub fn on_disconnect(&self) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
        metrics::gauge!("mailrs_connections_active").decrement(1.0);
    }

    pub fn on_message_delivered(&self) {
        self.total_messages.fetch_add(1, Ordering::Relaxed);
        metrics::counter!("mailrs_messages_total").increment(1);
    }
}

// shared types used across modules
