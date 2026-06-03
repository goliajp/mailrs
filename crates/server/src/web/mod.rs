use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

use crate::domain_store::DomainStore;
use crate::event_bus::EventBus;
use crate::health::HealthState;
use crate::inbound::auth_guard::AuthGuard;
use mailrs_mailbox::PgMailboxStore;

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

/// session token TTL: 7 days
const SESSION_TTL: Duration = Duration::from_secs(7 * 24 * 3600);

pub(crate) struct SessionInfo {
    address: String,
    display_name: String,
    permissions: Arc<crate::permission::EffectivePermissions>,
    created_at: Instant,
}

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
    pub outbound_queue: Option<sqlx::PgPool>,
    pub mailbox_store: Option<Arc<PgMailboxStore>>,
    pub domain_store: Option<Arc<DomainStore>>,
    pub maildir_root: String,
    pub hostname: String,
    pub sessions: DashMap<String, SessionInfo>,
    pub auth_guard: Option<Arc<AuthGuard>>,
    pub mta_sts_mode: Option<String>,
    pub mta_sts_mx: Vec<String>,
    pub mta_sts_max_age: u64,
    pub mta_sts_id: String,
    pub health: Option<HealthState>,
    pub pg_pool: Option<sqlx::PgPool>,
    pub kevy: Option<redis::aio::ConnectionManager>,
    /// In-process embed kevy store. Migration target — subsystems flip
    /// from `kevy` to this as they're ported off network kevy.
    pub kevy_embed: Option<crate::kevy_store::KevyStore>,
    pub llm_config: Option<Arc<dyn mailrs_intelligence::provider::LlmProvider>>,
    pub resolver: Option<Arc<hickory_resolver::TokioResolver>>,
    pub dkim_selector: Option<String>,
    pub smtp_config: Option<SmtpConfigSnapshot>,
    pub web_rate_limiter: Arc<rate_limit::WebRateLimiter>,
    pub ldap_config: Option<Arc<crate::ldap_auth::LdapConfig>>,
    pub oidc_config: Option<OidcConfig>,
    pub meili: Option<Arc<crate::search_index::MeiliClient>>,
    pub render_preview: Option<Arc<crate::render_preview::RenderPreviewClient>>,
    pub system_config: Option<Arc<crate::system_config::SystemConfigStore>>,
    /// Prometheus exporter handle for `/metrics` rendering. `None`
    /// only in unit tests that don't install the global recorder.
    pub metrics_handle: Option<metrics_exporter_prometheus::PrometheusHandle>,
}

/// spawn a background task to clean up expired sessions and stale rate-limit buckets every hour
pub fn spawn_session_cleanup(state: Arc<WebState>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(3600));
        loop {
            interval.tick().await;
            state
                .sessions
                .retain(|_, session| session.created_at.elapsed() < SESSION_TTL);
            // purge rate-limit buckets not seen in the last hour
            let stale_before_unix_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
                .saturating_sub(3600);
            state.web_rate_limiter.cleanup(stale_before_unix_secs).await;
            // clean up expired OIDC auth codes and refresh tokens
            if let Some(ref pool) = state.pg_pool {
                let _ = crate::oidc_store::cleanup_expired_codes(pool).await;
                let _ = crate::oidc_store::cleanup_expired_refresh_tokens(pool).await;
            }
            // clean up audit log entries older than 90 days
            if let Some(ref ds) = state.domain_store {
                ds.cleanup_audit_log(90).await;
            }
        }
    });
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
            maildir_root: String::new(),
            hostname: String::new(),
            sessions: DashMap::new(),
            auth_guard: None,
            mta_sts_mode: None,
            mta_sts_mx: vec![],
            mta_sts_max_age: 604800,
            mta_sts_id: String::new(),
            health: None,
            pg_pool: None,
            kevy: None,
            kevy_embed: None,
            llm_config: None,
            resolver: None,
            dkim_selector: None,
            smtp_config: None,
            web_rate_limiter: Arc::new(rate_limit::WebRateLimiter::new()),
            ldap_config: None,
            oidc_config: None,
            meili: None,
            render_preview: None,
            system_config: None,
            metrics_handle: None,
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

    pub fn with_queue(mut self, pool: sqlx::PgPool) -> Self {
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

    pub fn with_mta_sts(mut self, mode: String, mx: Vec<String>, max_age: u64, id: String) -> Self {
        self.mta_sts_mode = Some(mode);
        self.mta_sts_mx = mx;
        self.mta_sts_max_age = max_age;
        self.mta_sts_id = id;
        self
    }

    pub fn with_auth_guard(mut self, guard: Arc<AuthGuard>) -> Self {
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

    pub fn with_meili(mut self, client: Arc<crate::search_index::MeiliClient>) -> Self {
        self.meili = Some(client);
        self
    }

    pub fn with_render_preview(
        mut self,
        client: Arc<crate::render_preview::RenderPreviewClient>,
    ) -> Self {
        self.render_preview = Some(client);
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

    pub fn with_pg(mut self, pool: sqlx::PgPool) -> Self {
        self.pg_pool = Some(pool);
        self
    }

    pub fn with_kevy(mut self, conn: redis::aio::ConnectionManager) -> Self {
        self.kevy = Some(conn);
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

#[derive(Serialize)]
pub(crate) struct ApiResult {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct SendResult {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct DomainsQuery {
    #[serde(default)]
    pub domains: Option<String>,
}

pub(super) fn default_limit() -> u32 {
    50
}

/// maximum allowed limit for pagination queries
const MAX_LIMIT: u32 = 100;
/// maximum allowed offset for pagination queries
const MAX_OFFSET: u32 = 1_000_000;
/// maximum length for search query strings
const MAX_QUERY_LEN: usize = 500;
/// maximum length for path parameters (thread_id, folder name, etc.)
const MAX_PATH_LEN: usize = 512;
/// maximum number of items in a batch request
const MAX_BATCH_SIZE: usize = 100;
/// maximum number of recipients per send request
const MAX_RECIPIENTS: usize = 50;
/// maximum body size for multipart requests (25 MB)
const MAX_MULTIPART_BODY: usize = 25 * 1024 * 1024;
/// maximum length for admin string fields (domain name, address, etc.)
const MAX_ADMIN_FIELD_LEN: usize = 255;
/// maximum length for sieve scripts
const MAX_SIEVE_SCRIPT_LEN: usize = 64 * 1024;
/// maximum length for email body text in drafts/send
const MAX_EMAIL_BODY_LEN: usize = 512 * 1024;

/// clamp limit to MAX_LIMIT
pub(super) fn clamp_limit(limit: u32) -> u32 {
    limit.min(MAX_LIMIT)
}

/// clamp offset to MAX_OFFSET
pub(super) fn clamp_offset(offset: u32) -> u32 {
    offset.min(MAX_OFFSET)
}

/// parse and validate domains query parameter against user's accessible domains
pub(super) fn validate_domains(
    domains_param: Option<&str>,
    permissions: &crate::permission::EffectivePermissions,
) -> Option<Vec<String>> {
    let raw = domains_param?;
    if raw.is_empty() {
        return None;
    }

    let accessible = permissions.accessible_domains();
    if accessible.is_empty() {
        return None;
    }

    let requested: Vec<String> = raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // super users can access all requested domains
    if permissions.is_super() {
        return if requested.is_empty() {
            None
        } else {
            Some(requested)
        };
    }

    // only allow domains the user has permission for
    let validated: Vec<String> = requested
        .into_iter()
        .filter(|d| accessible.contains(d))
        .collect();

    if validated.is_empty() {
        None
    } else {
        Some(validated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- validate_domains ---

    fn make_perms(domains: &[&str]) -> crate::permission::EffectivePermissions {
        use crate::permission::{AccountGroup, GroupInfo, compute_effective_permissions};
        let groups: Vec<AccountGroup> = domains
            .iter()
            .map(|d| AccountGroup {
                group: GroupInfo {
                    id: 1,
                    name: "user".into(),
                    domain: Some(d.to_string()),
                    description: String::new(),
                    is_builtin: false,
                    created_at: 0,
                },
                permissions: vec!["mail.send".into(), "mail.read".into()],
            })
            .collect();
        compute_effective_permissions(&groups, &[], &[])
    }

    fn make_empty_perms() -> crate::permission::EffectivePermissions {
        crate::permission::compute_effective_permissions(&[], &[], &[])
    }

    #[test]
    fn validate_domains_returns_none_when_param_is_none() {
        assert!(validate_domains(None, &make_empty_perms()).is_none());
    }

    #[test]
    fn validate_domains_returns_none_when_param_is_empty() {
        assert!(validate_domains(Some(""), &make_empty_perms()).is_none());
    }

    #[test]
    fn validate_domains_returns_none_when_no_accessible_domains() {
        assert!(validate_domains(Some("example.com"), &make_empty_perms()).is_none());
    }

    #[test]
    fn validate_domains_returns_allowed_domain() {
        let perms = make_perms(&["example.com"]);
        let result = validate_domains(Some("example.com"), &perms);
        assert_eq!(result, Some(vec!["example.com".to_string()]));
    }

    #[test]
    fn validate_domains_filters_unauthorized_domains() {
        let perms = make_perms(&["example.com"]);
        let result = validate_domains(Some("example.com,evil.com"), &perms);
        assert_eq!(result, Some(vec!["example.com".to_string()]));
    }

    #[test]
    fn validate_domains_returns_none_when_all_domains_unauthorized() {
        let perms = make_perms(&["example.com"]);
        let result = validate_domains(Some("evil.com"), &perms);
        assert!(result.is_none());
    }

    #[test]
    fn validate_domains_handles_multiple_allowed_domains() {
        let perms = make_perms(&["example.com", "example.org"]);
        let result = validate_domains(Some("example.com,example.org"), &perms);
        assert_eq!(
            result,
            Some(vec!["example.com".to_string(), "example.org".to_string()])
        );
    }

    #[test]
    fn validate_domains_trims_whitespace() {
        let perms = make_perms(&["example.com"]);
        let result = validate_domains(Some("  example.com  ,  "), &perms);
        assert_eq!(result, Some(vec!["example.com".to_string()]));
    }

    #[test]
    fn validate_domains_skips_empty_segments() {
        let perms = make_perms(&["example.com"]);
        let result = validate_domains(Some(",example.com,,"), &perms);
        assert_eq!(result, Some(vec!["example.com".to_string()]));
    }

    // --- classify_email ---

    #[test]
    fn classify_safe_domain_noreply() {
        let (cat, score) = classify_email(
            "noreply@github.com",
            "You have a new notification",
            Some("Someone mentioned you in a PR"),
            None,
        );
        // noreply@ prefix triggers notification category even for safe domains
        assert_eq!(cat, "notification");
        assert_eq!(score, 0, "safe domain should have zero risk");
    }

    #[test]
    fn classify_notification_sender() {
        let (cat, score) = classify_email(
            "notifications@facebookmail.com",
            "You have a new friend request",
            Some("John wants to connect"),
            None,
        );
        assert_eq!(cat, "notification");
        assert!(score <= 15);
    }

    #[test]
    fn classify_promotion_with_unsubscribe() {
        let (cat, _score) = classify_email(
            "offers@shop.example.com",
            "Big Summer Sale!",
            Some("Check our latest deals. Click to unsubscribe"),
            None,
        );
        assert_eq!(cat, "promotion");
    }

    #[test]
    fn classify_promotion_with_marketing_keywords() {
        let (cat, _score) = classify_email(
            "news@store.example.com",
            "Newsletter: Special Discount",
            Some("Check our latest newsletter. Click to unsubscribe."),
            None,
        );
        assert_eq!(cat, "promotion");
    }

    #[test]
    fn classify_spam_multiple_signals() {
        let (cat, score) = classify_email(
            "unknown@sketchy.example.com",
            "URGENT: You are a winner!",
            Some("Click here to claim your prize. Act now, limited time!"),
            None,
        );
        assert!(
            cat == "spam" || cat == "scam",
            "expected spam or scam, got {cat}"
        );
        assert!(score >= 40, "spam score should be >= 40, got {score}");
    }

    #[test]
    fn classify_scam_phishing_signals() {
        let (cat, score) = classify_email(
            "security@phisher.example.com",
            "Your account has been suspended",
            Some(
                "Login immediately to verify your account. Confirm your identity. Your password needs updating.",
            ),
            None,
        );
        assert_eq!(cat, "scam");
        assert!(score >= 60, "phishing score should be >= 60, got {score}");
    }

    #[test]
    fn classify_detects_tracking_pixels() {
        let (cat, _score) = classify_email(
            "info@tracker.example.com",
            "Weekly Update",
            Some("Here is your update"),
            Some(
                "<html><body><img src='https://t.example.com/px' width=\"1\" height=\"1\" /></body></html>",
            ),
        );
        assert_eq!(cat, "promotion");
    }

    #[test]
    fn classify_detects_many_links() {
        let links = "<a href='#'>link</a>".repeat(25);
        let html = format!("<html><body>{links}</body></html>");
        let (cat, score) = classify_email(
            "info@newsletter.example.com",
            "Links roundup",
            None,
            Some(&html),
        );
        assert!(score >= 5, "many links should add to score, got {score}");
        assert!(cat == "promotion" || cat == "general");
    }

    #[test]
    fn classify_plain_personal_email() {
        let (cat, score) = classify_email(
            "friend@personal.example.com",
            "Dinner tonight?",
            Some("Hey, want to grab dinner at 7pm?"),
            None,
        );
        assert_eq!(cat, "personal");
        assert_eq!(score, 0);
    }

    #[test]
    fn classify_general_email_with_low_score() {
        let (cat, score) = classify_email(
            "support@company.example.com",
            "Your ticket has been updated",
            Some("We have an update on your support ticket #12345"),
            None,
        );
        assert!(cat == "personal" || cat == "general");
        assert!(score < 40);
    }

    #[test]
    fn classify_safe_domain_resists_spam_signals() {
        let (cat, score) = classify_email(
            "noreply@google.com",
            "Urgent: verify your account",
            Some("Please verify your account"),
            None,
        );
        assert!(score < 60, "safe domain should dampen score, got {score}");
        assert_ne!(cat, "scam");
    }

    #[test]
    fn classify_japanese_spam_signals() {
        let (cat, score) = classify_email(
            "unknown@example.com",
            "至急ご確認ください",
            Some("当選おめでとうございます。緊急のお知らせです。"),
            None,
        );
        assert!(
            score >= 40,
            "japanese spam signals should raise score, got {score}"
        );
        assert!(cat == "spam" || cat == "scam");
    }

    #[test]
    fn classify_chinese_phishing_signals() {
        let (cat, score) = classify_email(
            "security@example.com",
            "账户异常通知",
            Some("您的账号被锁定，请立即修改密码"),
            None,
        );
        assert!(
            score >= 40,
            "chinese phish signals should raise score, got {score}"
        );
        assert!(cat == "spam" || cat == "scam");
    }

    #[test]
    fn classify_notification_with_noreply_prefix() {
        let (cat, score) = classify_email(
            "noreply@some-service.example.com",
            "Your order has shipped",
            Some("Your package is on the way"),
            None,
        );
        assert_eq!(cat, "notification");
        assert!(score <= 15);
    }

    #[test]
    fn classify_score_clamped_to_100() {
        let (_, score) = classify_email(
            "scammer@evil.example.com",
            "URGENT: winner! congratulations! lottery prize!",
            Some(
                "click here, act now, limited time, verify your account, suspended, locked, password, login immediately, confirm your identity, アカウントが制限, アカウントを確認, 账户异常, 账号被锁, 密码, パスワード, 当選, 至急, 緊急, 中奖, 恭喜, 紧急",
            ),
            None,
        );
        assert!(score <= 100, "score should be clamped to 100, got {score}");
    }

    #[test]
    fn classify_score_never_negative() {
        let (_, score) = classify_email(
            "noreply@github.com",
            "PR review requested",
            Some("Please review this pull request"),
            None,
        );
        assert_eq!(score, 0);
    }

    #[test]
    fn classify_html_unsubscribe_in_html_only() {
        let (cat, _score) = classify_email(
            "news@example.com",
            "Monthly Report",
            None,
            Some("<html><body><p>Report content</p><a href='#'>unsubscribe</a></body></html>"),
        );
        assert_eq!(cat, "promotion");
    }

    #[test]
    fn classify_case_insensitive() {
        let (cat1, score1) = classify_email(
            "NOREPLY@GITHUB.COM",
            "PR Review",
            Some("Please review"),
            None,
        );
        let (cat2, score2) = classify_email(
            "noreply@github.com",
            "PR Review",
            Some("Please review"),
            None,
        );
        assert_eq!(cat1, cat2);
        assert_eq!(score1, score2);
    }
}
