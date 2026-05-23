use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::middleware;
use axum::routing::{any, delete, get, post, put};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;

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
mod conversations;
mod dav;
mod jmap;
pub(crate) mod mail;
mod oidc_provider;
mod templates;
mod system_config;
mod webhook;
pub(crate) mod rate_limit;
mod request_id;
mod rsvp;
mod ws;

pub(crate) use auth::{AuthMethod, AuthUser};

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
    pub valkey: Option<redis::aio::ConnectionManager>,
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
            valkey: None,
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
        }
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

    pub fn with_render_preview(mut self, client: Arc<crate::render_preview::RenderPreviewClient>) -> Self {
        self.render_preview = Some(client);
        self
    }

    pub fn with_system_config(mut self, store: Arc<crate::system_config::SystemConfigStore>) -> Self {
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

    pub fn with_valkey(mut self, conn: redis::aio::ConnectionManager) -> Self {
        self.valkey = Some(conn);
        self
    }

    pub fn on_connect(&self) {
        self.total_connections.fetch_add(1, Ordering::Relaxed);
        self.active_connections.fetch_add(1, Ordering::Relaxed);
    }

    pub fn on_disconnect(&self) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn on_message_delivered(&self) {
        self.total_messages.fetch_add(1, Ordering::Relaxed);
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

/// classify an email: category + risk score (0=safe .. 100=dangerous)
pub(super) fn classify_email(
    sender: &str,
    subject: &str,
    text: Option<&str>,
    html: Option<&str>,
) -> (String, u8) {
    let sender_lc = sender.to_lowercase();
    let subject_lc = subject.to_lowercase();
    let text_lc = text.unwrap_or("").to_lowercase();
    let html_lc = html.unwrap_or("").to_lowercase();
    let all = format!("{sender_lc} {subject_lc} {text_lc}");

    let mut score: i32 = 0;

    // known safe senders (personal, business, dev)
    let safe_domains = [
        "github.com",
        "noreply.github.com",
        "gitlab.com",
        "freee.co.jp",
        "atcoder.jp",
        "apple.com",
        "google.com",
        "golia.jp",
        "golia.ai",
    ];
    let is_safe_domain = safe_domains.iter().any(|d| sender_lc.contains(d));
    if is_safe_domain {
        score -= 30;
    }

    // advertising signals
    let ad_signals = [
        "unsubscribe",
        "配信停止",
        "メール配信",
        "opt-out",
        "list-unsubscribe",
        "配信解除",
        "退订",
        "取消订阅",
        "email preferences",
    ];
    let ad_count = ad_signals
        .iter()
        .filter(|s| all.contains(*s) || html_lc.contains(*s))
        .count();

    // newsletter / marketing patterns
    let marketing_signals = [
        "newsletter",
        "ニュースレター",
        "pr】",
        "＜pr＞",
        "お知らせ",
        "セール",
        "キャンペーン",
        "クーポン",
        "ポイント",
        "おすすめ",
        "sale",
        "discount",
        "promotion",
        "deal",
        "offer",
        "特価",
        "限定",
        "タイムセール",
        "お得",
    ];
    let marketing_count = marketing_signals
        .iter()
        .filter(|s| all.contains(*s))
        .count();

    // spam signals
    let spam_signals = [
        "click here",
        "act now",
        "limited time",
        "winner",
        "congratulations",
        "lottery",
        "prize",
        "urgent",
        "verify your account",
        "suspended",
        "locked",
        "当選",
        "至急",
        "緊急",
        "中奖",
        "恭喜",
        "紧急",
    ];
    let spam_count = spam_signals.iter().filter(|s| all.contains(*s)).count();

    // phishing signals
    let phish_signals = [
        "password",
        "パスワード",
        "密码",
        "login immediately",
        "confirm your identity",
        "アカウントが制限",
        "アカウントを確認",
        "账户异常",
        "账号被锁",
    ];
    let phish_count = phish_signals.iter().filter(|s| all.contains(*s)).count();

    // technical signals (tracking pixels, many links, hidden text)
    let has_tracking = html_lc.contains("width=\"1\"")
        || html_lc.contains("width:1px")
        || html_lc.contains("height=\"1\"")
        || html_lc.contains("height:1px");
    let link_count = html_lc.matches("<a ").count();

    score += ad_count as i32 * 5;
    score += marketing_count as i32 * 8;
    score += spam_count as i32 * 20;
    score += phish_count as i32 * 25;
    if has_tracking {
        score += 5;
    }
    if link_count > 20 {
        score += 5;
    }

    // known notification senders (low risk)
    let notification_domains = [
        "facebookmail.com",
        "linkedin.com",
        "substack.com",
        "steampowered.com",
        "quora.com",
        "tripadvisor.com",
        "noreply@",
        "no-reply@",
        "notification",
    ];
    let is_notification = notification_domains.iter().any(|d| sender_lc.contains(d));
    if is_notification && score < 30 {
        score = score.min(15);
    }

    let score = score.clamp(0, 100) as u8;

    let category = if score >= 60 {
        "scam"
    } else if score >= 40 {
        "spam"
    } else if ad_count > 0 || marketing_count >= 2 || has_tracking {
        "promotion"
    } else if is_notification {
        "notification"
    } else if is_safe_domain || score == 0 {
        "personal"
    } else {
        "general"
    };

    (category.to_string(), score)
}

/// middleware that adds security headers to all responses
async fn security_headers(
    request: axum::extract::Request,
    next: middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::X_CONTENT_TYPE_OPTIONS,
        "nosniff".parse().unwrap(),
    );
    headers.insert(
        axum::http::header::X_FRAME_OPTIONS,
        "DENY".parse().unwrap(),
    );
    headers.insert(
        axum::http::header::X_XSS_PROTECTION,
        "0".parse().unwrap(),
    );
    headers.insert(
        axum::http::header::REFERRER_POLICY,
        "strict-origin-when-cross-origin".parse().unwrap(),
    );
    headers.insert(
        axum::http::HeaderName::from_static("permissions-policy"),
        "camera=(), microphone=(), geolocation=()".parse().unwrap(),
    );
    // CSP: strict script-src, inline styles for tailwind, data: images for
    // embedded email content, websocket via connect-src, srcdoc iframes via
    // frame-src 'self', and base-uri/form-action lockdown
    headers.insert(
        axum::http::header::CONTENT_SECURITY_POLICY,
        concat!(
            "default-src 'self'; ",
            "script-src 'self'; ",
            "style-src 'self' 'unsafe-inline'; ",
            "img-src 'self' data:; ",
            "font-src 'self'; ",
            "connect-src 'self'; ",
            "frame-src 'self'; ",
            "base-uri 'self'; ",
            "form-action 'self'",
        )
            .parse()
            .unwrap(),
    );
    response
}

pub fn router(state: Arc<WebState>, static_dir: Option<&str>) -> axum::Router {
    let rate_limiter = state.web_rate_limiter.clone();

    // mcp router: auth middleware but no general rate limiter (MCP sessions are long-lived)
    let mcp_router = crate::mcp::setup_mcp(state.clone())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::mcp::auth::mcp_auth_middleware,
        ))
        .layer(middleware::from_fn(security_headers))
        .layer(
            CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods([
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    axum::http::Method::DELETE,
                    axum::http::Method::OPTIONS,
                ])
                .allow_headers([
                    axum::http::header::AUTHORIZATION,
                    axum::http::header::CONTENT_TYPE,
                ])
                .max_age(Duration::from_secs(3600)),
        );

    // auth routes with stricter rate limit (10 req/min per IP)
    let auth_routes = axum::Router::new()
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/forgot-password", post(auth::forgot_password))
        .route("/api/auth/reset-password", post(auth::reset_password))
        .route_layer(middleware::from_fn_with_state(
            rate_limiter.clone(),
            rate_limit::auth_rate_limit,
        ));

    let mut app = axum::Router::new()
        // status + health
        .route("/api/status", get(admin::get_status))
        .route("/api/health", get(admin::get_health))
        .route("/api/readiness", get(admin::get_readiness))
        .route("/metrics", get(admin::prometheus_metrics))
        // websocket
        .route("/api/events", get(ws::ws_events))
        // queue
        .route("/api/queue", get(admin::get_queue))
        .route("/api/queue/{id}/retry", post(admin::retry_queue_message))
        // mail API
        .route(
            "/api/calendar/conflicts",
            get(calendar_api::get_conflicts),
        )
        .route(
            "/api/calendar/feeds",
            get(calendar_api::list_feeds).post(calendar_api::create_feed),
        )
        .route(
            "/api/calendar/feeds/{feed_id}",
            axum::routing::delete(calendar_api::delete_feed),
        )
        .route(
            "/api/invites/{message_id}/rsvp",
            post(rsvp::submit_rsvp),
        )
        .route(
            "/api/invites/{message_id}/counter",
            post(rsvp::submit_counter),
        )
        .route("/api/mail/folders", get(mail::get_folders))
        .route(
            "/api/mail/folders/{name}/messages",
            get(mail::get_folder_messages),
        )
        .route("/api/mail/messages/{uid}", get(mail::get_message))
        .route(
            "/api/mail/messages/{uid}/flags",
            post(mail::update_message_flags),
        )
        .route("/api/mail/messages/{uid}", delete(mail::delete_message))
        .route("/api/mail/export", get(mail::export_mbox))
        .route("/api/mail/send", post(mail::send_message))
        .route("/api/mail/check-deliverability", post(mail::check_deliverability))
        .route("/api/mail/spam-feedback", post(mail::submit_spam_feedback))
        .route("/api/mail/render-preview", post(mail::render_preview))
        .route("/api/mail/render-preview/cache/{id}", get(mail::serve_render_cache))
        .route("/api/admin/spam-feedback-stats", get(mail::get_spam_feedback_stats))
        .route(
            "/api/mail/send-multipart",
            post(mail::send_message_multipart),
        )
        .route(
            "/api/mail/pending/{message_id}",
            delete(mail::cancel_pending_send),
        )
        .route(
            "/api/mail/messages/{uid}/raw",
            get(mail::get_message_raw),
        )
        .route(
            "/api/mail/messages/{uid}/attachments/{index}",
            get(mail::get_attachment),
        )
        // attachment content (OCR/PDF text)
        .route(
            "/api/mail/messages/{uid}/attachments/{index}/content",
            get(mail::get_attachment_content),
        )
        // inline image upload/serve
        .route(
            "/api/mail/inline-upload",
            post(mail::upload_inline_image),
        )
        .route(
            "/api/mail/inline/{id}",
            get(mail::serve_inline_image),
        )
        // drafts API
        .route(
            "/api/mail/drafts",
            post(mail::save_draft).get(mail::list_drafts),
        )
        .route("/api/mail/drafts/{id}", delete(mail::delete_draft))
        // signatures API
        .route(
            "/api/mail/signatures",
            post(mail::save_signature).get(mail::list_signatures),
        )
        .route(
            "/api/mail/signatures/{id}",
            delete(mail::delete_signature),
        )
        // encryption keys API
        .route("/api/mail/keys", get(mail::list_keys))
        .route(
            "/api/mail/keys/{key_type}",
            get(mail::get_key).put(mail::set_key).delete(mail::delete_key),
        )
        // public key lookup (no auth required, rate-limited by general_rate_limit layer)
        .route("/api/keys/{address}/pgp", get(mail::get_public_pgp_key))
        .route("/api/keys/{address}/smime", get(mail::get_public_smime_key))
        // templates API
        .route(
            "/api/mail/templates",
            post(templates::save_template).get(templates::list_templates),
        )
        .route(
            "/api/mail/templates/{id}",
            delete(templates::delete_template),
        )
        // AI assist
        .route("/api/mail/ai/polish", post(ai_assist::ai_polish))
        .route("/api/mail/ai/reply-suggest", post(ai_assist::ai_reply_suggest))
        .route("/api/mail/ai/generate-subject", post(ai_assist::ai_generate_subject))
        // conversations API
        .route("/api/conversations", get(conversations::get_conversations))
        .route(
            "/api/conversations/batch",
            post(conversations::batch_conversations),
        )
        .route(
            "/api/conversations/categories",
            get(conversations::get_conversation_categories),
        )
        .route(
            "/api/conversations/action-count",
            get(conversations::get_action_count),
        )
        .route(
            "/api/conversations/search",
            get(conversations::search_conversations),
        )
        .route(
            "/api/conversations/semantic-search",
            get(conversations::semantic_search),
        )
        .route(
            "/api/conversations/{thread_id}",
            get(conversations::get_thread_messages).delete(conversations::delete_thread),
        )
        .route(
            "/api/conversations/{thread_id}/read",
            post(conversations::mark_thread_read),
        )
        .route(
            "/api/conversations/{thread_id}/unread",
            post(conversations::mark_thread_unread),
        )
        .route(
            "/api/conversations/{thread_id}/star",
            post(conversations::star_thread),
        )
        .route(
            "/api/conversations/{thread_id}/unstar",
            post(conversations::unstar_thread),
        )
        .route(
            "/api/conversations/{thread_id}/pin",
            post(conversations::pin_thread),
        )
        .route(
            "/api/conversations/{thread_id}/unpin",
            post(conversations::unpin_thread),
        )
        .route(
            "/api/conversations/{thread_id}/dismiss-action",
            post(conversations::dismiss_action),
        )
        .route(
            "/api/conversations/{thread_id}/archive",
            post(conversations::archive_thread),
        )
        .route(
            "/api/conversations/{thread_id}/unarchive",
            post(conversations::unarchive_thread),
        )
        .route(
            "/api/conversations/{thread_id}/snooze",
            put(conversations::snooze_thread).delete(conversations::unsnooze_thread),
        )
        .route(
            "/api/conversations/{thread_id}/messages/{uid}/reactions",
            put(conversations::toggle_reaction),
        )
        .route(
            "/api/conversations/{thread_id}/reactions",
            get(conversations::get_thread_reactions),
        )
        .route("/api/contacts", get(conversations::get_contacts))
        .route("/api/mail/stats", get(conversations::get_mail_stats))
        .route(
            "/api/mail/feedback",
            post(conversations::record_feedback),
        )
        // auth API (login handled separately with stricter rate limit)
        .merge(auth_routes)
        .route("/api/auth/logout", post(auth::logout))
        .route("/api/auth/me", get(auth::auth_me))
        // self-service password change
        .route("/api/auth/change-password", post(auth::change_password))
        // recovery email
        .route("/api/auth/recovery-email", get(auth::get_recovery_email).post(auth::update_recovery_email))
        // identity verification (for external IdPs)
        .route("/api/auth/verify", post(auth::verify_credentials))
        .route("/api/auth/verify-totp", post(auth::verify_totp))
        // OIDC client (Sign in with GOLIA)
        .route("/api/auth/oidc/login", get(auth::oidc_login))
        .route("/api/auth/oidc/callback", get(auth::oidc_callback))
        .route("/api/auth/oidc/config", get(auth::oidc_client_config))
        // TOTP 2FA
        .route("/api/auth/totp/setup", post(auth::totp_setup))
        .route("/api/auth/totp/enable", post(auth::totp_enable))
        .route("/api/auth/totp/disable", post(auth::totp_disable))
        .route("/api/auth/totp/status", get(auth::totp_status))
        // API key management
        .route("/api/agent/keys", post(api_key::create_api_key).get(api_key::list_api_keys))
        .route("/api/agent/keys/{id}", delete(api_key::revoke_api_key))
        // webhook subscriptions
        .route("/api/agent/webhooks", post(webhook::create_webhook).get(webhook::list_webhooks))
        .route("/api/agent/webhooks/{id}", delete(webhook::delete_webhook))
        // admin API
        .route(
            "/api/admin/domains",
            get(admin::list_domains).post(admin::add_domain),
        )
        .route("/api/admin/domains/{name}", delete(admin::remove_domain))
        .route(
            "/api/admin/domains/{name}/check",
            post(admin::check_domain_handler),
        )
        .route("/api/admin/rbl-status", get(admin::get_rbl_status))
        .route("/api/admin/reputation", get(admin::get_reputation))
        .route("/api/admin/export", get(admin::export_messages))
        .route(
            "/api/admin/suppressions",
            get(admin::list_suppressed).delete(admin::remove_suppressed),
        )
        .route(
            "/api/admin/accounts",
            get(admin::list_accounts).post(admin::add_account),
        )
        .route(
            "/api/admin/accounts/{address}",
            put(admin::update_account).delete(admin::remove_account),
        )
        .route(
            "/api/admin/aliases",
            get(admin::list_aliases).post(admin::add_alias),
        )
        .route("/api/admin/aliases/{id}", delete(admin::remove_alias))
        // quota + sieve
        .route(
            "/api/admin/accounts/{address}/quota",
            get(admin::get_quota).post(admin::set_quota),
        )
        .route(
            "/api/admin/accounts/{address}/sieve",
            get(admin::get_sieve)
                .post(admin::set_sieve)
                .delete(admin::delete_sieve),
        )
        // groups CRUD
        .route(
            "/api/admin/groups",
            get(admin::list_groups).post(admin::create_group),
        )
        .route("/api/admin/groups/{id}", delete(admin::delete_group))
        .route(
            "/api/admin/groups/{id}/permissions",
            get(admin::get_group_permissions).put(admin::set_group_permissions),
        )
        .route(
            "/api/admin/groups/{id}/members",
            get(admin::list_group_members).post(admin::add_group_member),
        )
        .route(
            "/api/admin/groups/{id}/members/{address}",
            delete(admin::remove_group_member),
        )
        .route(
            "/api/admin/accounts/{address}/groups",
            get(admin::get_account_groups),
        )
        .route(
            "/api/admin/accounts/{address}/overrides",
            get(admin::get_account_overrides).put(admin::set_account_overrides),
        )
        .route("/api/admin/permissions", get(admin::get_all_permissions))
        // email groups
        .route(
            "/api/admin/email-groups",
            get(admin::list_email_groups).post(admin::create_email_group),
        )
        .route(
            "/api/admin/email-groups/{id}",
            delete(admin::delete_email_group),
        )
        .route(
            "/api/admin/email-groups/{id}/members",
            get(admin::list_email_group_members).post(admin::add_email_group_member),
        )
        .route(
            "/api/admin/email-groups/{id}/members/{address}",
            delete(admin::remove_email_group_member),
        )
        // apps
        .route(
            "/api/admin/apps",
            get(admin::list_apps).post(admin::create_app),
        )
        .route(
            "/api/admin/apps/{app_id}",
            get(admin::get_app).delete(admin::delete_app),
        )
        .route(
            "/api/admin/apps/{app_id}/scopes",
            put(admin::update_app_scopes),
        )
        // audit log
        .route("/api/admin/audit-log", get(admin::get_audit_log))
        // mail audit (admin impersonate)
        .route("/api/admin/audit/accounts", get(admin::audit_list_accounts))
        .route("/api/admin/audit/conversations", get(admin::audit_list_conversations))
        .route(
            "/api/admin/audit/conversations/{thread_id}/messages",
            get(admin::audit_get_thread_messages),
        )
        .route(
            "/api/admin/audit/messages/{uid}/raw",
            get(admin::audit_get_raw_message),
        )
        // smtp config
        .route("/api/admin/config/smtp", get(admin::get_smtp_config))
        // system config (runtime-editable)
        .route("/api/admin/system-config", get(system_config::list_config))
        .route("/api/admin/system-config/{key}", put(system_config::update_config).delete(system_config::reset_config))
        // JMAP
        .route("/.well-known/jmap", get(jmap::jmap_session))
        .route("/jmap", post(jmap::jmap_api))
        .route("/jmap/eventsource/", get(jmap::jmap_eventsource))
        // OIDC provider
        .route("/.well-known/openid-configuration", get(oidc_provider::openid_configuration))
        .route("/.well-known/jwks.json", get(oidc_provider::jwks))
        .route("/oauth/authorize", get(oidc_provider::authorize))
        .route("/oauth/token", post(oidc_provider::token))
        .route("/oauth/userinfo", get(oidc_provider::userinfo))
        // OAuth client admin
        .route(
            "/api/admin/oauth-clients",
            post(admin::create_oauth_client).get(admin::list_oauth_clients),
        )
        .route("/api/admin/oauth-clients/{client_id}", delete(admin::delete_oauth_client))
        // MTA-STS policy
        .route("/.well-known/mta-sts.txt", get(admin::mta_sts_policy))
        // mail client autodiscover
        .route(
            "/autodiscover/autodiscover.xml",
            post(autodiscover::autodiscover_outlook),
        )
        .route(
            "/Autodiscover/Autodiscover.xml",
            post(autodiscover::autodiscover_outlook),
        )
        .route(
            "/.well-known/autoconfig/mail/config-v1.1.xml",
            get(autodiscover::autoconfig_mozilla),
        )
        .route(
            "/mail/config-v1.1.xml",
            get(autodiscover::autoconfig_mozilla),
        )
        // CalDAV / CardDAV (well-known redirects + DAV endpoints)
        .route("/.well-known/caldav", any(dav::well_known_caldav))
        .route("/.well-known/carddav", any(dav::well_known_carddav))
        .route("/dav/", any(dav::dav_principal))
        .route("/dav/calendars/{user}/", any(dav::dav_calendar_home))
        .route("/dav/calendars/{user}/{calendar}/", any(dav::dav_calendar_collection))
        .route("/dav/calendars/{user}/{calendar}/{uid}", any(dav::dav_event))
        .route("/dav/contacts/{user}/", any(dav::dav_contact_home))
        .route("/dav/contacts/{user}/{book}/", any(dav::dav_contact_collection))
        .route("/dav/contacts/{user}/{book}/{uid}", any(dav::dav_contact))
        .layer(axum::extract::DefaultBodyLimit::max(MAX_MULTIPART_BODY))
        .layer(middleware::from_fn(request_id::request_id_middleware))
        .layer(middleware::from_fn_with_state(
            rate_limiter,
            rate_limit::general_rate_limit,
        ))
        .layer(middleware::from_fn(security_headers))
        .layer(
            CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods([
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    axum::http::Method::PUT,
                    axum::http::Method::PATCH,
                    axum::http::Method::DELETE,
                    axum::http::Method::OPTIONS,
                ])
                .allow_headers([
                    axum::http::header::AUTHORIZATION,
                    axum::http::header::CONTENT_TYPE,
                    axum::http::HeaderName::from_static("x-request-id"),
                ])
                .expose_headers([axum::http::HeaderName::from_static("x-request-id")])
                .max_age(Duration::from_secs(3600)),
        )
        .with_state(state.clone());

    // merge MCP router after with_state so it bypasses the general rate limiter
    app = app.merge(mcp_router.with_state(state.clone()));

    // BIMI logo lookup — bypasses rate limiter (cached DNS, read-only)
    // Image proxy — fetches external email images through our server (requires auth)
    let bimi_router = axum::Router::new()
        .route("/api/bimi/{domain}", get(mail::get_bimi_logo))
        .route("/api/proxy/image", get(mail::proxy_image))
        .route("/api/proxy/link", get(mail::proxy_link))
        .with_state(state);
    app = app.merge(bimi_router);

    // serve frontend static files with SPA fallback
    if let Some(dir) = static_dir {
        use tower_http::services::{ServeDir, ServeFile};
        let index = format!("{dir}/index.html");
        app = app.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)));
    }

    // HTTP-request-level tracing span. Wraps EVERY route (including the
    // post-with_state merges above) so all per-handler log lines + any
    // future #[instrument] handlers nest under one `web.req` span per
    // request. Span carries method + URI; status code + latency are added
    // on response.
    app = app.layer(
        tower_http::trace::TraceLayer::new_for_http().make_span_with(
            |req: &axum::http::Request<_>| {
                tracing::info_span!(
                    "web.req",
                    method = %req.method(),
                    uri = %req.uri(),
                )
            },
        ),
    );

    app
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- validate_domains ---

    fn make_perms(domains: &[&str]) -> crate::permission::EffectivePermissions {
        use crate::permission::{compute_effective_permissions, AccountGroup, GroupInfo};
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
            Some("Login immediately to verify your account. Confirm your identity. Your password needs updating."),
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
            Some("<html><body><img src='https://t.example.com/px' width=\"1\" height=\"1\" /></body></html>"),
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
        assert!(score >= 40, "japanese spam signals should raise score, got {score}");
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
        assert!(score >= 40, "chinese phish signals should raise score, got {score}");
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
            Some("click here, act now, limited time, verify your account, suspended, locked, password, login immediately, confirm your identity, アカウントが制限, アカウントを確認, 账户异常, 账号被锁, 密码, パスワード, 当選, 至急, 緊急, 中奖, 恭喜, 紧急"),
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
