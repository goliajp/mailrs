use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use std::time::Instant;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{ConnectInfo, FromRequestParts, Multipart, Path, Query, State, WebSocketUpgrade};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::Json;
use base64::Engine;
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;

use mail_parser::MimeHeaders;
use rand_core::RngCore;

use crate::domain_store::DomainStore;
use crate::event_bus::EventBus;
use crate::health::HealthState;
use crate::inbound::auth_guard::{AuthCheck, AuthGuard};
use crate::message_util::{self, AttachmentInfo};
use mailrs_mailbox::MailboxStore;

pub(crate) struct SessionInfo {
    address: String,
    display_name: String,
}

pub struct WebState {
    pub event_bus: EventBus,
    pub started_at: Instant,
    pub total_connections: AtomicU64,
    pub total_messages: AtomicU64,
    pub active_connections: AtomicU64,
    pub outbound_queue: Option<sqlx::PgPool>,
    pub mailbox_store: Option<Arc<MailboxStore>>,
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
    pub gemini_config: Option<crate::ai_email::GeminiConfig>,
    pub resolver: Option<Arc<hickory_resolver::TokioResolver>>,
    pub dkim_selector: Option<String>,
}

impl WebState {
    pub fn new(event_bus: EventBus) -> Self {
        Self {
            event_bus,
            started_at: Instant::now(),
            total_connections: AtomicU64::new(0),
            total_messages: AtomicU64::new(0),
            active_connections: AtomicU64::new(0),
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
            gemini_config: None,
            resolver: None,
            dkim_selector: None,
        }
    }

    pub fn with_gemini(mut self, config: crate::ai_email::GeminiConfig) -> Self {
        self.gemini_config = Some(config);
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

    pub fn with_mailbox(mut self, store: Arc<MailboxStore>) -> Self {
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

#[derive(Serialize)]
struct StatusResponse {
    uptime_secs: u64,
    active_connections: u64,
    total_connections: u64,
    total_messages: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    queue: Option<QueueStatsResp>,
}

#[derive(Serialize)]
struct QueueStatsResp {
    pending: i64,
    inflight: i64,
    delivered: i64,
    failed: i64,
    bounced: i64,
}

#[derive(Serialize)]
struct QueueEntry {
    id: i64,
    sender: String,
    recipient: String,
    domain: String,
    status: String,
    attempts: u32,
    last_error: Option<String>,
    created_at: i64,
    updated_at: i64,
}

#[derive(Serialize)]
struct RetryResponse {
    success: bool,
    message: String,
}

// mail API types
#[derive(Serialize)]
struct FolderInfo {
    name: String,
    total: u32,
    unseen: u32,
    uidnext: u32,
}

#[derive(Serialize)]
struct MessageSummary {
    uid: u32,
    sender: String,
    recipients: String,
    subject: String,
    size: u32,
    flags: u32,
    internal_date: i64,
}

#[derive(Serialize)]
struct MessageDetail {
    uid: u32,
    sender: String,
    recipients: String,
    subject: String,
    size: u32,
    flags: u32,
    internal_date: i64,
    text_body: Option<String>,
    html_body: Option<String>,
    attachments: Vec<AttachmentInfo>,
    category: String,
    risk_score: u8,
    risk_reason: String,
    summary: String,
    people: serde_json::Value,
    dates: serde_json::Value,
    amounts: serde_json::Value,
    action_items: serde_json::Value,
    ai_analyzed: bool,
    clean_text: Option<String>,
}

#[derive(Deserialize)]
struct FolderMessagesQuery {
    #[serde(default = "default_limit")]
    limit: u32,
    #[serde(default)]
    offset: u32,
}

fn default_limit() -> u32 {
    50
}

#[derive(Deserialize)]
struct FlagUpdate {
    action: String, // "add", "remove", "set"
    flags: u32,
}

// admin API types
#[derive(Deserialize)]
struct AddDomainRequest {
    name: String,
}

#[derive(Deserialize)]
struct AddAccountRequest {
    address: String,
    domain: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    password: String,
}

#[derive(Serialize)]
struct ApiResult {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

// auth types
#[derive(Deserialize)]
struct LoginRequest {
    address: String,
    password: String,
}

/// extractor that validates bearer token and returns the user address
struct AuthUser(String);

impl FromRequestParts<Arc<WebState>> for AuthUser {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<WebState>,
    ) -> Result<Self, Self::Rejection> {
        // check Authorization header
        let auth_header = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if let Some(token) = auth_header.strip_prefix("Bearer ") {
            if let Some(session) = state.sessions.get(token) {
                return Ok(AuthUser(session.address.clone()));
            }
        }

        // fallback: check ?user= query param (dev compatibility)
        let query = parts.uri.query().unwrap_or("");
        for pair in query.split('&') {
            if let Some(user) = pair.strip_prefix("user=") {
                let user = urlencoding::decode(user).unwrap_or_default().into_owned();
                if !user.is_empty() {
                    return Ok(AuthUser(user));
                }
            }
        }

        Err((StatusCode::UNAUTHORIZED, "authentication required"))
    }
}

// conversation API types
#[derive(Serialize)]
struct ConversationResponse {
    thread_id: String,
    subject: String,
    participants: Vec<String>,
    message_count: u32,
    unread_count: u32,
    last_date: i64,
    category: String,
}

#[derive(Serialize)]
struct ThreadMessageResponse {
    id: i64,
    uid: u32,
    sender: String,
    recipients: String,
    subject: String,
    flags: u32,
    internal_date: i64,
    message_id: String,
    text_body: Option<String>,
    html_body: Option<String>,
    attachments: Vec<AttachmentInfo>,
    category: String,
    risk_score: u8,
    risk_reason: String,
    summary: String,
    people: serde_json::Value,
    dates: serde_json::Value,
    amounts: serde_json::Value,
    action_items: serde_json::Value,
    ai_analyzed: bool,
    clean_text: Option<String>,
}

/// classify an email: category + risk score (0=safe .. 100=dangerous)
fn classify_email(sender: &str, subject: &str, text: Option<&str>, html: Option<&str>) -> (String, u8) {
    let sender_lc = sender.to_lowercase();
    let subject_lc = subject.to_lowercase();
    let text_lc = text.unwrap_or("").to_lowercase();
    let html_lc = html.unwrap_or("").to_lowercase();
    let all = format!("{sender_lc} {subject_lc} {text_lc}");

    let mut score: i32 = 0;

    // known safe senders (personal, business, dev)
    let safe_domains = [
        "github.com", "noreply.github.com", "gitlab.com",
        "freee.co.jp", "atcoder.jp", "apple.com",
        "google.com", "golia.jp", "golia.ai",
    ];
    let is_safe_domain = safe_domains.iter().any(|d| sender_lc.contains(d));
    if is_safe_domain { score -= 30; }

    // advertising signals
    let ad_signals = [
        "unsubscribe", "配信停止", "メール配信", "opt-out", "list-unsubscribe",
        "配信解除", "退订", "取消订阅", "email preferences",
    ];
    let ad_count = ad_signals.iter().filter(|s| all.contains(*s) || html_lc.contains(*s)).count();

    // newsletter / marketing patterns
    let marketing_signals = [
        "newsletter", "ニュースレター", "pr】", "＜pr＞", "お知らせ",
        "セール", "キャンペーン", "クーポン", "ポイント", "おすすめ",
        "sale", "discount", "promotion", "deal", "offer",
        "特価", "限定", "タイムセール", "お得",
    ];
    let marketing_count = marketing_signals.iter().filter(|s| all.contains(*s)).count();

    // spam signals
    let spam_signals = [
        "click here", "act now", "limited time", "winner",
        "congratulations", "lottery", "prize", "urgent",
        "verify your account", "suspended", "locked",
        "当選", "至急", "緊急",
        "中奖", "恭喜", "紧急",
    ];
    let spam_count = spam_signals.iter().filter(|s| all.contains(*s)).count();

    // phishing signals
    let phish_signals = [
        "password", "パスワード", "密码",
        "login immediately", "confirm your identity",
        "アカウントが制限", "アカウントを確認",
        "账户异常", "账号被锁",
    ];
    let phish_count = phish_signals.iter().filter(|s| all.contains(*s)).count();

    // technical signals (tracking pixels, many links, hidden text)
    let has_tracking = html_lc.contains("width=\"1\"") || html_lc.contains("width:1px")
        || html_lc.contains("height=\"1\"") || html_lc.contains("height:1px");
    let link_count = html_lc.matches("<a ").count();

    score += ad_count as i32 * 5;
    score += marketing_count as i32 * 8;
    score += spam_count as i32 * 20;
    score += phish_count as i32 * 25;
    if has_tracking { score += 5; }
    if link_count > 20 { score += 5; }

    // known notification senders (low risk)
    let notification_domains = [
        "facebookmail.com", "linkedin.com", "substack.com",
        "steampowered.com", "quora.com", "tripadvisor.com",
        "noreply@", "no-reply@", "notification",
    ];
    let is_notification = notification_domains.iter().any(|d| sender_lc.contains(d));
    if is_notification && score < 30 { score = score.min(15); }

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

#[derive(Deserialize)]
struct ConversationsQuery {
    #[serde(default = "default_limit")]
    limit: u32,
    #[serde(default)]
    before: Option<i64>,
    #[serde(default)]
    category: Option<String>,
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    #[serde(default = "default_limit")]
    limit: u32,
    #[serde(default)]
    category: Option<String>,
}

#[derive(Deserialize)]
struct ContactsQuery {
    #[serde(default)]
    q: String,
    #[serde(default = "default_contacts_limit")]
    limit: u32,
}

fn default_contacts_limit() -> u32 {
    20
}

// compose/send types
#[derive(Deserialize)]
struct SendMessageRequest {
    from: String,
    to: Vec<String>,
    #[serde(default)]
    cc: Vec<String>,
    #[serde(default)]
    bcc: Vec<String>,
    subject: String,
    body: String,
    #[serde(default)]
    in_reply_to: Option<String>,
    #[serde(default)]
    list_unsubscribe: Option<String>,
}

pub fn router(state: Arc<WebState>, static_dir: Option<&str>) -> axum::Router {
    let mut app = axum::Router::new()
        // existing endpoints
        .route("/api/status", get(get_status))
        .route("/api/health", get(get_health))
        .route("/api/events", get(ws_events))
        .route("/api/queue", get(get_queue))
        .route("/api/queue/{id}/retry", post(retry_queue_message))
        // mail API
        .route("/api/mail/folders", get(get_folders))
        .route("/api/mail/folders/{name}/messages", get(get_folder_messages))
        .route("/api/mail/messages/{uid}", get(get_message))
        .route("/api/mail/messages/{uid}/flags", post(update_message_flags))
        .route("/api/mail/messages/{uid}", delete(delete_message))
        .route("/api/mail/send", post(send_message))
        .route("/api/mail/send-multipart", post(send_message_multipart))
        .route("/api/mail/messages/{uid}/attachments/{index}", get(get_attachment))
        // conversations API
        .route("/api/conversations", get(get_conversations))
        .route("/api/conversations/categories", get(get_conversation_categories))
        .route("/api/conversations/search", get(search_conversations))
        .route("/api/conversations/semantic-search", get(semantic_search))
        .route("/api/conversations/{thread_id}", get(get_thread_messages))
        .route("/api/conversations/{thread_id}/read", post(mark_thread_read))
        .route("/api/contacts", get(get_contacts))
        // auth API
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/me", get(auth_me))
        // admin API
        .route("/api/admin/domains", get(list_domains).post(add_domain))
        .route("/api/admin/domains/{name}", delete(remove_domain))
        .route("/api/admin/domains/{name}/check", post(check_domain_handler))
        .route("/api/admin/accounts", get(list_accounts).post(add_account))
        .route("/api/admin/accounts/{address}", delete(remove_account))
        .route("/api/admin/aliases", get(list_aliases).post(add_alias))
        .route("/api/admin/aliases/{id}", delete(remove_alias))
        // quota + sieve
        .route("/api/admin/accounts/{address}/quota", get(get_quota).post(set_quota))
        .route("/api/admin/accounts/{address}/sieve", get(get_sieve).post(set_sieve).delete(delete_sieve))
        // MTA-STS policy
        .route("/.well-known/mta-sts.txt", get(mta_sts_policy))
        // mail client autodiscover
        .route("/autodiscover/autodiscover.xml", post(autodiscover_outlook))
        .route("/Autodiscover/Autodiscover.xml", post(autodiscover_outlook))
        .route("/.well-known/autoconfig/mail/config-v1.1.xml", get(autoconfig_mozilla))
        .route("/mail/config-v1.1.xml", get(autoconfig_mozilla))
        .layer(CorsLayer::permissive())
        .with_state(state);

    // serve frontend static files with SPA fallback
    if let Some(dir) = static_dir {
        use tower_http::services::{ServeDir, ServeFile};
        let index = format!("{dir}/index.html");
        app = app.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)));
    }

    app
}

// ---------- status + queue endpoints ----------

async fn get_status(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let queue = if let Some(ref pool) = state.outbound_queue {
        match mailrs_outbound_queue::queue::queue_stats(pool).await {
            Ok(stats) => {
                let mut qs = QueueStatsResp {
                    pending: 0,
                    inflight: 0,
                    delivered: 0,
                    failed: 0,
                    bounced: 0,
                };
                for (status, count) in stats {
                    match status.as_str() {
                        "pending" => qs.pending = count,
                        "inflight" => qs.inflight = count,
                        "delivered" => qs.delivered = count,
                        "failed" => qs.failed = count,
                        "bounced" => qs.bounced = count,
                        _ => {}
                    }
                }
                Some(qs)
            }
            Err(_) => None,
        }
    } else {
        None
    };

    Json(StatusResponse {
        uptime_secs: state.started_at.elapsed().as_secs(),
        active_connections: state.active_connections.load(Ordering::Relaxed),
        total_connections: state.total_connections.load(Ordering::Relaxed),
        total_messages: state.total_messages.load(Ordering::Relaxed),
        queue,
    })
}

async fn get_health(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let (level, pg, valkey) = match &state.health {
        Some(h) => (h.level(), h.pg_up(), h.valkey_up()),
        None => (3, false, false),
    };
    let status = if level == 0 {
        StatusCode::OK
    } else {
        StatusCode::OK // degraded but still serving
    };
    (
        status,
        Json(serde_json::json!({
            "level": level,
            "pg": pg,
            "valkey": valkey,
        })),
    )
}

async fn get_queue(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let Some(ref pool) = state.outbound_queue else {
        return Json(Vec::<QueueEntry>::new());
    };

    let entries = match mailrs_outbound_queue::queue::list_recent(pool, 100).await {
        Ok(msgs) => msgs.into_iter().map(|m| QueueEntry {
            id: m.id,
            sender: m.sender,
            recipient: m.recipient,
            domain: m.domain,
            status: m.status.as_str().to_string(),
            attempts: m.attempts,
            last_error: m.last_error,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }).collect(),
        Err(_) => vec![],
    };

    Json(entries)
}

async fn retry_queue_message(
    Path(id): Path<i64>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref pool) = state.outbound_queue else {
        return Json(RetryResponse {
            success: false,
            message: "outbound queue not configured".into(),
        });
    };

    let now = chrono::Utc::now().timestamp();
    match mailrs_outbound_queue::queue::retry_message(pool, id, now).await {
        Ok(true) => Json(RetryResponse {
            success: true,
            message: format!("message {id} queued for retry"),
        }),
        Ok(false) => Json(RetryResponse {
            success: false,
            message: format!("message {id} not found or not retryable"),
        }),
        Err(e) => Json(RetryResponse {
            success: false,
            message: format!("error: {e}"),
        }),
    }
}

// ---------- auth endpoints ----------

async fn login(
    State(state): State<Arc<WebState>>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    Json(req): Json<LoginRequest>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "auth not configured"})),
        );
    };

    // check auth guard before attempting verification
    if let Some(ref guard) = state.auth_guard {
        if let AuthCheck::LockedOut { remaining_secs } = guard.check(addr.ip(), &req.address) {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({
                    "error": format!("Too many auth failures, try again in {remaining_secs}s")
                })),
            );
        }
    }

    let (account, password_hash) = match ds.get_account_with_hash(&req.address).await {
        Ok(Some(pair)) => pair,
        _ => {
            if let Some(ref guard) = state.auth_guard {
                guard.record_failure(addr.ip(), &req.address);
            }
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "invalid credentials"})),
            );
        }
    };

    if !account.active {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "account disabled"})),
        );
    }

    // verify password
    let valid = if password_hash.starts_with("$argon2") {
        crate::users::UserStore::verify_hash(&req.password, &password_hash)
    } else {
        password_hash == req.password
    };

    if !valid {
        if let Some(ref guard) = state.auth_guard {
            guard.record_failure(addr.ip(), &req.address);
        }
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid credentials"})),
        );
    }

    if let Some(ref guard) = state.auth_guard {
        guard.record_success(addr.ip(), &req.address);
    }

    // generate token
    let mut bytes = [0u8; 32];
    rand_core::OsRng.fill_bytes(&mut bytes);
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();

    state.sessions.insert(
        token.clone(),
        SessionInfo {
            address: account.address.clone(),
            display_name: account.display_name.clone(),
        },
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "token": token,
            "address": account.address,
            "display_name": account.display_name,
        })),
    )
}

async fn logout(
    State(state): State<Arc<WebState>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    if let Some(token) = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        state.sessions.remove(token);
    }
    Json(ApiResult {
        success: true,
        message: None,
    })
}

async fn auth_me(
    State(state): State<Arc<WebState>>,
    AuthUser(user): AuthUser,
) -> impl IntoResponse {
    // find display name from session or domain store
    let display_name = state
        .sessions
        .iter()
        .find(|s| s.value().address == user)
        .map(|s| s.value().display_name.clone())
        .unwrap_or_default();

    Json(serde_json::json!({
        "address": user,
        "display_name": display_name,
    }))
}

// ---------- mail API endpoints ----------

async fn get_folders(
    AuthUser(user): AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<FolderInfo>::new());
    };

    // auto-create default mailboxes on first access
    let _ = mb_store.ensure_default_mailboxes(&user).await;

    let mailboxes = mb_store.list_mailboxes(&user).await.unwrap_or_default();
    let mut folders = Vec::with_capacity(mailboxes.len());
    for mb in &mailboxes {
        let (total, unseen) = mb_store.mailbox_status(mb.id).await.unwrap_or((0, 0));
        folders.push(FolderInfo {
            name: mb.name.clone(),
            total,
            unseen,
            uidnext: mb.uidnext,
        });
    }

    Json(folders)
}

async fn get_folder_messages(
    Path(name): Path<String>,
    Query(q): Query<FolderMessagesQuery>,
    AuthUser(user): AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<MessageSummary>::new());
    };

    let mb = match mb_store.get_mailbox(&user, &name).await {
        Ok(Some(mb)) => mb,
        _ => return Json(Vec::<MessageSummary>::new()),
    };

    let messages = mb_store
        .list_messages(mb.id, q.offset, q.limit)
        .await
        .unwrap_or_default();

    let summaries: Vec<MessageSummary> = messages
        .iter()
        .map(|msg| MessageSummary {
            uid: msg.uid,
            sender: message_util::decode_header(&msg.sender),
            recipients: msg.recipients.clone(),
            subject: message_util::decode_header(&msg.subject),
            size: msg.size,
            flags: msg.flags,
            internal_date: msg.internal_date,
        })
        .collect();

    Json(summaries)
}

async fn get_message(
    Path(uid): Path<u32>,
    AuthUser(user): AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(None::<MessageDetail>);
    };

    // find the message across all mailboxes
    let mailboxes = mb_store.list_mailboxes(&user).await.unwrap_or_default();
    for mb in &mailboxes {
        if let Ok(Some(msg)) = mb_store.get_message(mb.id, uid).await {
            let raw = message_util::read_message_raw(&state.maildir_root, &user, &msg.maildir_id);
            let parsed = raw.as_deref().map(message_util::parse_message).unwrap_or_default();
            let sender = message_util::decode_header(&msg.sender);
            let subject = message_util::decode_header(&msg.subject);

            // try AI analysis first, fall back to rule-based
            let ai = mb_store.get_email_analysis(msg.id).await.ok().flatten();
            let (category, risk_score, risk_reason, summary, people, dates, amounts, action_items, ai_analyzed, clean_text) =
                if let Some(ref a) = ai {
                    let ct = if a.clean_text.is_empty() { None } else { Some(a.clean_text.clone()) };
                    (
                        a.category.clone(),
                        a.risk_score as u8,
                        a.risk_reason.clone(),
                        a.summary.clone(),
                        a.people.clone(),
                        a.dates.clone(),
                        a.amounts.clone(),
                        a.action_items.clone(),
                        true,
                        ct,
                    )
                } else {
                    let (cat, score) = classify_email(&sender, &subject, parsed.0.as_deref(), parsed.1.as_deref());
                    (cat, score, String::new(), String::new(), serde_json::json!([]), serde_json::json!([]), serde_json::json!([]), serde_json::json!([]), false, None)
                };

            return Json(Some(MessageDetail {
                uid: msg.uid,
                sender,
                recipients: msg.recipients,
                subject,
                size: msg.size,
                flags: msg.flags,
                internal_date: msg.internal_date,
                text_body: parsed.0,
                html_body: parsed.1,
                attachments: parsed.2,
                category,
                risk_score,
                risk_reason,
                summary,
                people,
                dates,
                amounts,
                action_items,
                ai_analyzed,
                clean_text,
            }));
        }
    }

    Json(None::<MessageDetail>)
}

async fn update_message_flags(
    Path(uid): Path<u32>,
    AuthUser(user): AuthUser,
    State(state): State<Arc<WebState>>,
    Json(update): Json<FlagUpdate>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    let mailboxes = mb_store.list_mailboxes(&user).await.unwrap_or_default();
    for mb in &mailboxes {
        if mb_store.get_message(mb.id, uid).await.ok().flatten().is_some() {
            let result = match update.action.as_str() {
                "add" => mb_store.add_flags(mb.id, uid, update.flags).await,
                "remove" => mb_store.remove_flags(mb.id, uid, update.flags).await,
                _ => mb_store.update_flags(mb.id, uid, update.flags).await,
            };
            return Json(ApiResult {
                success: result.is_ok(),
                message: result.err().map(|e| e.to_string()),
            });
        }
    }

    Json(ApiResult {
        success: false,
        message: Some("message not found".into()),
    })
}

async fn delete_message(
    Path(uid): Path<u32>,
    AuthUser(user): AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    // mark as deleted
    let mailboxes = mb_store.list_mailboxes(&user).await.unwrap_or_default();
    for mb in &mailboxes {
        if mb_store.get_message(mb.id, uid).await.ok().flatten().is_some() {
            let result = mb_store.add_flags(mb.id, uid, mailrs_mailbox::FLAG_DELETED).await;
            return Json(ApiResult {
                success: result.is_ok(),
                message: result.err().map(|e| e.to_string()),
            });
        }
    }

    Json(ApiResult {
        success: false,
        message: Some("message not found".into()),
    })
}

async fn send_message(
    AuthUser(user): AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SendMessageRequest>,
) -> impl IntoResponse {
    if req.to.is_empty() {
        return Json(ApiResult {
            success: false,
            message: Some("to is required".into()),
        });
    }

    // use authenticated user as sender
    let from = if req.from.is_empty() { &user } else { &req.from };

    // verify sender matches authenticated user
    if from != &user {
        return Json(ApiResult {
            success: false,
            message: Some("sender must match authenticated user".into()),
        });
    }

    let now = chrono::Utc::now();
    let message_id = format!(
        "{}.{}@{}",
        now.timestamp_millis(),
        rand_core::OsRng.next_u32(),
        state.hostname
    );

    // build full References chain from thread history
    let references = match (req.in_reply_to.as_deref(), state.mailbox_store.as_ref()) {
        (Some(reply_to), Some(mb_store)) if !reply_to.is_empty() => {
            mb_store.get_thread_references(from, reply_to).await.unwrap_or_default()
        }
        _ => vec![],
    };

    // append quoted text from original message for replies
    let body_with_quote = match (req.in_reply_to.as_deref(), state.mailbox_store.as_ref()) {
        (Some(reply_to), Some(mb_store)) if !reply_to.is_empty() => {
            if let Some(orig) = mb_store.find_message_by_message_id(from, reply_to).await.ok().flatten() {
                if let Some(raw_orig) = message_util::read_message_raw(&state.maildir_root, from, &orig.maildir_id) {
                    let (text_body, _, _) = message_util::parse_message(&raw_orig);
                    if let Some(text) = text_body {
                        let sender = message_util::decode_header(&orig.sender);
                        let date = chrono::DateTime::from_timestamp(orig.internal_date, 0)
                            .map(|dt| dt.format("%a, %d %b %Y %H:%M").to_string())
                            .unwrap_or_default();
                        let quoted: String = text.lines().map(|l| format!("> {l}\n")).collect();
                        format!("{}\n\nOn {date}, {sender} wrote:\n{quoted}", req.body)
                    } else {
                        req.body.clone()
                    }
                } else {
                    req.body.clone()
                }
            } else {
                req.body.clone()
            }
        }
        _ => req.body.clone(),
    };

    let raw = build_rfc5322_message(
        from,
        &req.to,
        &req.cc,
        &req.subject,
        &body_with_quote,
        &message_id,
        req.in_reply_to.as_deref(),
        &references,
        &now,
        req.list_unsubscribe.as_deref(),
    );

    deliver_message(&state, from, &req.to, &req.cc, &req.bcc, &raw, &message_id, now.timestamp()).await
}

async fn deliver_message(
    state: &Arc<WebState>,
    from: &str,
    to: &[String],
    cc: &[String],
    bcc: &[String],
    raw: &[u8],
    message_id: &str,
    ts: i64,
) -> Json<ApiResult> {
    let all_recipients: Vec<&str> = to
        .iter()
        .chain(cc.iter())
        .chain(bcc.iter())
        .map(|s| s.as_str())
        .collect();

    let local_domains: Vec<String> = if let Some(ref ds) = state.domain_store {
        ds.list_domains().await.unwrap_or_default().into_iter().map(|d| d.name).collect()
    } else {
        vec![]
    };

    let mut errors = Vec::new();

    for rcpt in &all_recipients {
        let domain = rcpt.rsplit_once('@').map(|(_, d)| d).unwrap_or("");
        let is_local = local_domains.iter().any(|d: &String| d.eq_ignore_ascii_case(domain));

        if is_local {
            if let Some(ref mb_store) = state.mailbox_store {
                let _ = mb_store.ensure_default_mailboxes(rcpt).await;
                if let Err(e) = mb_store.append_message(
                    rcpt, "INBOX", &state.maildir_root, raw, 0, ts,
                ).await {
                    errors.push(format!("{rcpt}: {e}"));
                }
            }
        } else if let Some(ref pool) = state.outbound_queue {
            if let Err(e) = mailrs_outbound_queue::queue::enqueue(
                pool, from, rcpt, domain, raw, Some(message_id), ts,
            ).await {
                errors.push(format!("{rcpt}: {e}"));
            } else if let Some(ref vk) = state.valkey {
                mailrs_outbound_queue::queue::notify(&mut vk.clone()).await;
            }
        } else {
            errors.push(format!("{rcpt}: outbound queue not configured"));
        }
    }

    // save copy to Sent folder
    if let Some(ref mb_store) = state.mailbox_store {
        let _ = mb_store.ensure_default_mailboxes(from).await;
        let _ = mb_store.append_message(
            from, "Sent", &state.maildir_root, raw, mailrs_mailbox::FLAG_SEEN, ts,
        ).await;
    }

    if errors.is_empty() {
        Json(ApiResult { success: true, message: None })
    } else {
        Json(ApiResult { success: false, message: Some(errors.join("; ")) })
    }
}

struct AttachmentData {
    filename: String,
    content_type: String,
    data: Vec<u8>,
}

fn build_rfc5322_message(
    from: &str,
    to: &[String],
    cc: &[String],
    subject: &str,
    body: &str,
    message_id: &str,
    in_reply_to: Option<&str>,
    references: &[String],
    date: &chrono::DateTime<chrono::Utc>,
    list_unsubscribe: Option<&str>,
) -> Vec<u8> {
    build_rfc5322_with_attachments(from, to, cc, subject, body, message_id, in_reply_to, references, date, &[], list_unsubscribe)
}

fn build_rfc5322_with_attachments(
    from: &str,
    to: &[String],
    cc: &[String],
    subject: &str,
    body: &str,
    message_id: &str,
    in_reply_to: Option<&str>,
    references: &[String],
    date: &chrono::DateTime<chrono::Utc>,
    attachments: &[AttachmentData],
    list_unsubscribe: Option<&str>,
) -> Vec<u8> {
    let date_str = date.format("%a, %d %b %Y %H:%M:%S %z").to_string();
    let mut msg = format!(
        "Date: {date_str}\r\n\
         From: {from}\r\n\
         To: {}\r\n",
        to.join(", ")
    );
    if !cc.is_empty() {
        msg.push_str(&format!("Cc: {}\r\n", cc.join(", ")));
    }
    msg.push_str(&format!(
        "Subject: {subject}\r\n\
         Message-ID: <{message_id}>\r\n\
         MIME-Version: 1.0\r\n"
    ));
    if let Some(ref_id) = in_reply_to {
        msg.push_str(&format!("In-Reply-To: <{ref_id}>\r\n"));
    }
    if !references.is_empty() {
        let refs_str = references.iter().map(|r| format!("<{r}>")).collect::<Vec<_>>().join(" ");
        msg.push_str(&format!("References: {refs_str}\r\n"));
    } else if let Some(ref_id) = in_reply_to {
        msg.push_str(&format!("References: <{ref_id}>\r\n"));
    }
    if let Some(unsub_url) = list_unsubscribe {
        msg.push_str(&format!("List-Unsubscribe: <{unsub_url}>\r\n"));
        msg.push_str("List-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n");
    }

    if attachments.is_empty() {
        msg.push_str("Content-Type: text/plain; charset=utf-8\r\n");
        msg.push_str("Content-Transfer-Encoding: 8bit\r\n");
        msg.push_str("\r\n");
        msg.push_str(body);
    } else {
        let boundary = format!("----=_Part_{}", rand_core::OsRng.next_u64());
        msg.push_str(&format!(
            "Content-Type: multipart/mixed; boundary=\"{boundary}\"\r\n\r\n"
        ));

        // text part
        msg.push_str(&format!("--{boundary}\r\n"));
        msg.push_str("Content-Type: text/plain; charset=utf-8\r\n");
        msg.push_str("Content-Transfer-Encoding: 8bit\r\n\r\n");
        msg.push_str(body);
        msg.push_str("\r\n");

        // attachment parts
        for att in attachments {
            msg.push_str(&format!("--{boundary}\r\n"));
            msg.push_str(&format!(
                "Content-Type: {}; name=\"{}\"\r\n",
                att.content_type, att.filename
            ));
            msg.push_str("Content-Transfer-Encoding: base64\r\n");
            msg.push_str(&format!(
                "Content-Disposition: attachment; filename=\"{}\"\r\n\r\n",
                att.filename
            ));

            let encoded = base64::engine::general_purpose::STANDARD.encode(&att.data);
            // wrap at 76 chars per RFC 2045
            for chunk in encoded.as_bytes().chunks(76) {
                msg.push_str(std::str::from_utf8(chunk).unwrap_or(""));
                msg.push_str("\r\n");
            }
        }

        msg.push_str(&format!("--{boundary}--\r\n"));
    }

    msg.into_bytes()
}

async fn send_message_multipart(
    AuthUser(user): AuthUser,
    State(state): State<Arc<WebState>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let mut from = String::new();
    let mut to: Vec<String> = Vec::new();
    let mut cc: Vec<String> = Vec::new();
    let mut subject = String::new();
    let mut body = String::new();
    let mut in_reply_to: Option<String> = None;
    let mut attachments: Vec<AttachmentData> = Vec::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "from" => from = field.text().await.unwrap_or_default(),
            "to" => to.push(field.text().await.unwrap_or_default()),
            "cc" => cc.push(field.text().await.unwrap_or_default()),
            "subject" => subject = field.text().await.unwrap_or_default(),
            "body" => body = field.text().await.unwrap_or_default(),
            "in_reply_to" => {
                let val = field.text().await.unwrap_or_default();
                if !val.is_empty() {
                    in_reply_to = Some(val);
                }
            }
            "attachments" => {
                let filename = field
                    .file_name()
                    .unwrap_or("unnamed")
                    .to_string();
                let content_type = field
                    .content_type()
                    .unwrap_or("application/octet-stream")
                    .to_string();
                if let Ok(data) = field.bytes().await {
                    attachments.push(AttachmentData {
                        filename,
                        content_type,
                        data: data.to_vec(),
                    });
                }
            }
            _ => {}
        }
    }

    if from.is_empty() {
        from = user.clone();
    }

    if from != user {
        return Json(ApiResult {
            success: false,
            message: Some("sender must match authenticated user".into()),
        });
    }

    if to.is_empty() {
        return Json(ApiResult {
            success: false,
            message: Some("to is required".into()),
        });
    }

    let now = chrono::Utc::now();
    let message_id = format!(
        "{}.{}@{}",
        now.timestamp_millis(),
        rand_core::OsRng.next_u32(),
        state.hostname
    );

    // build full References chain from thread history
    let references = match (in_reply_to.as_deref(), state.mailbox_store.as_ref()) {
        (Some(reply_to), Some(mb_store)) if !reply_to.is_empty() => {
            mb_store.get_thread_references(&from, reply_to).await.unwrap_or_default()
        }
        _ => vec![],
    };

    // append quoted text from original message for replies
    let body_with_quote = match (in_reply_to.as_deref(), state.mailbox_store.as_ref()) {
        (Some(reply_to), Some(mb_store)) if !reply_to.is_empty() => {
            if let Some(orig) = mb_store.find_message_by_message_id(&from, reply_to).await.ok().flatten() {
                if let Some(raw_orig) = message_util::read_message_raw(&state.maildir_root, &from, &orig.maildir_id) {
                    let (text_body, _, _) = message_util::parse_message(&raw_orig);
                    if let Some(text) = text_body {
                        let sender = message_util::decode_header(&orig.sender);
                        let date = chrono::DateTime::from_timestamp(orig.internal_date, 0)
                            .map(|dt| dt.format("%a, %d %b %Y %H:%M").to_string())
                            .unwrap_or_default();
                        let quoted: String = text.lines().map(|l| format!("> {l}\n")).collect();
                        format!("{body}\n\nOn {date}, {sender} wrote:\n{quoted}")
                    } else {
                        body
                    }
                } else {
                    body
                }
            } else {
                body
            }
        }
        _ => body,
    };

    let raw = build_rfc5322_with_attachments(
        &from,
        &to,
        &cc,
        &subject,
        &body_with_quote,
        &message_id,
        in_reply_to.as_deref(),
        &references,
        &now,
        &attachments,
        None, // multipart send doesn't support list-unsubscribe
    );

    deliver_message(&state, &from, &to, &cc, &[], &raw, &message_id, now.timestamp()).await
}

// ---------- conversation API endpoints ----------

async fn get_conversations(
    AuthUser(user): AuthUser,
    Query(q): Query<ConversationsQuery>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<ConversationResponse>::new());
    };

    let _ = mb_store.ensure_default_mailboxes(&user).await;

    let convos = mb_store
        .list_conversations(&user, q.limit, q.before, q.category.as_deref())
        .await
        .unwrap_or_default();

    Json(convos_to_response(convos))
}

fn convos_to_response(convos: Vec<mailrs_mailbox::ConversationSummary>) -> Vec<ConversationResponse> {
    convos
        .into_iter()
        .map(|c| ConversationResponse {
            thread_id: c.thread_id,
            subject: message_util::decode_header(&c.subject),
            participants: c
                .participants
                .split(',')
                .map(|s| message_util::decode_header(s.trim()))
                .collect(),
            message_count: c.message_count,
            unread_count: c.unread_count,
            last_date: c.last_date,
            category: c.category,
        })
        .collect()
}

async fn get_thread_messages(
    Path(thread_id): Path<String>,
    AuthUser(user): AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<ThreadMessageResponse>::new());
    };

    let messages = mb_store
        .list_thread_messages(&user, &thread_id)
        .await
        .unwrap_or_default();

    let mut result = Vec::with_capacity(messages.len());
    for msg in &messages {
        let raw = message_util::read_message_raw(&state.maildir_root, &user, &msg.maildir_id);
        let parsed = raw.as_deref().map(message_util::parse_message).unwrap_or_default();

        // fallback: extract sender/subject from raw email if DB values are empty
        let (sender, subject) = if msg.sender.is_empty() || msg.subject.is_empty() {
            let raw_sender = raw
                .as_deref()
                .map(|d| message_util::extract_header_from_raw(d, "From"))
                .unwrap_or_default();
            let raw_subject = raw
                .as_deref()
                .map(|d| message_util::extract_header_from_raw(d, "Subject"))
                .unwrap_or_default();
            (
                if msg.sender.is_empty() { message_util::decode_header(&raw_sender) } else { message_util::decode_header(&msg.sender) },
                if msg.subject.is_empty() { message_util::decode_header(&raw_subject) } else { message_util::decode_header(&msg.subject) },
            )
        } else {
            (message_util::decode_header(&msg.sender), message_util::decode_header(&msg.subject))
        };

        // try AI analysis first, fall back to rule-based
        let ai = mb_store.get_email_analysis(msg.id).await.ok().flatten();
        let (category, risk_score, risk_reason, summary, people, dates, amounts, action_items, ai_analyzed, clean_text) =
            if let Some(ref a) = ai {
                let ct = if a.clean_text.is_empty() { None } else { Some(a.clean_text.clone()) };
                (
                    a.category.clone(),
                    a.risk_score as u8,
                    a.risk_reason.clone(),
                    a.summary.clone(),
                    a.people.clone(),
                    a.dates.clone(),
                    a.amounts.clone(),
                    a.action_items.clone(),
                    true,
                    ct,
                )
            } else {
                let (cat, score) = classify_email(&sender, &subject, parsed.0.as_deref(), parsed.1.as_deref());
                (cat, score, String::new(), String::new(), serde_json::json!([]), serde_json::json!([]), serde_json::json!([]), serde_json::json!([]), false, None)
            };

        result.push(ThreadMessageResponse {
            id: msg.id,
            uid: msg.uid,
            sender,
            recipients: msg.recipients.clone(),
            subject,
            flags: msg.flags,
            internal_date: msg.internal_date,
            message_id: msg.message_id.clone(),
            text_body: parsed.0,
            html_body: parsed.1,
            attachments: parsed.2,
            category,
            risk_score,
            risk_reason,
            summary,
            people,
            dates,
            amounts,
            action_items,
            ai_analyzed,
            clean_text,
        });
    }

    Json(result)
}

async fn mark_thread_read(
    Path(thread_id): Path<String>,
    AuthUser(user): AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    match mb_store.mark_thread_read(&user, &thread_id).await {
        Ok(count) => Json(ApiResult {
            success: true,
            message: Some(format!("{count} messages marked as read")),
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}

#[derive(Serialize)]
struct CategoryCount {
    category: String,
    count: i64,
}

async fn get_conversation_categories(
    AuthUser(user): AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<CategoryCount>::new());
    };

    let cats = mb_store
        .list_conversation_categories(&user)
        .await
        .unwrap_or_default();

    let result: Vec<CategoryCount> = cats
        .into_iter()
        .map(|(category, count)| CategoryCount { category, count })
        .collect();

    Json(result)
}

async fn search_conversations(
    AuthUser(user): AuthUser,
    Query(q): Query<SearchQuery>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<ConversationResponse>::new());
    };

    let mut convos = mb_store
        .search_conversations(&user, &q.q, q.limit, q.category.as_deref())
        .await
        .unwrap_or_default();

    // supplement with semantic search when text search returns few results
    if convos.len() < q.limit as usize {
        if let Some(extra) = semantic_search_threads(&state, &user, &q.q, q.limit as usize - convos.len(), q.category.as_deref()).await {
            let existing: std::collections::HashSet<String> =
                convos.iter().map(|c| c.thread_id.clone()).collect();
            for c in extra {
                if !existing.contains(&c.thread_id) {
                    convos.push(c);
                }
            }
        }
    }

    Json(convos_to_response(convos))
}

/// run semantic search and build ConversationSummary for each matching thread
async fn semantic_search_threads(
    state: &WebState,
    user: &str,
    query: &str,
    max: usize,
    category: Option<&str>,
) -> Option<Vec<mailrs_mailbox::ConversationSummary>> {
    let gemini = state.gemini_config.as_ref()?;
    let mb = state.mailbox_store.as_ref()?;

    let embedding = crate::ai_email::generate_embedding(gemini, query).await?;
    let results = mb.semantic_search(user, &embedding, max.min(20) as i64).await.ok()?;

    let mut out = Vec::new();
    for (_, thread_id, _) in &results {
        let msgs = mb.list_thread_messages(user, thread_id).await.ok()?;
        let first = msgs.first()?;
        let last = msgs.last().unwrap();

        let cat = mb.get_email_analysis(last.id).await.ok().flatten()
            .map(|a| a.category)
            .unwrap_or_else(|| "general".to_string());

        if let Some(filter) = category {
            if cat != filter { continue; }
        }

        let participants: Vec<String> = msgs.iter()
            .map(|m| m.sender.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        out.push(mailrs_mailbox::ConversationSummary {
            thread_id: thread_id.clone(),
            subject: first.subject.clone(),
            participants: participants.join(","),
            message_count: msgs.len() as u32,
            unread_count: msgs.iter().filter(|m| m.flags & 1 == 0).count() as u32,
            last_date: last.internal_date,
            category: cat,
        });
    }

    Some(out)
}

#[derive(Serialize)]
struct SemanticSearchResult {
    thread_id: String,
    similarity: f64,
}

async fn semantic_search(
    AuthUser(user): AuthUser,
    Query(q): Query<SearchQuery>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<SemanticSearchResult>::new());
    };
    let Some(ref gemini_config) = state.gemini_config else {
        return Json(Vec::<SemanticSearchResult>::new());
    };

    // generate embedding for the query
    let embedding = match crate::ai_email::generate_embedding(gemini_config, &q.q).await {
        Some(e) => e,
        None => return Json(Vec::<SemanticSearchResult>::new()),
    };

    let results = mb_store
        .semantic_search(&user, &embedding, q.limit as i64)
        .await
        .unwrap_or_default();

    // deduplicate by thread_id, keep highest similarity
    let mut seen = std::collections::HashMap::new();
    for (_, thread_id, similarity) in &results {
        let entry = seen.entry(thread_id.clone()).or_insert(*similarity);
        if *similarity > *entry {
            *entry = *similarity;
        }
    }

    let result: Vec<SemanticSearchResult> = seen
        .into_iter()
        .map(|(thread_id, similarity)| SemanticSearchResult { thread_id, similarity })
        .collect();

    Json(result)
}

async fn get_contacts(
    AuthUser(user): AuthUser,
    Query(q): Query<ContactsQuery>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<String>::new());
    };

    let contacts = mb_store
        .search_contacts(&user, &q.q, q.limit)
        .await
        .unwrap_or_default();

    Json(contacts)
}

async fn get_attachment(
    Path((uid, index)): Path<(u32, usize)>,
    AuthUser(user): AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return (
            StatusCode::NOT_FOUND,
            [("content-type", "text/plain".to_string()), ("content-disposition", String::new())],
            Vec::new(),
        );
    };

    let mailboxes = mb_store.list_mailboxes(&user).await.unwrap_or_default();
    for mb in &mailboxes {
        if let Ok(Some(msg)) = mb_store.get_message(mb.id, uid).await {
            let raw = message_util::read_message_raw(&state.maildir_root, &user, &msg.maildir_id);
            if let Some(data) = raw {
                if let Some(parsed) = mail_parser::MessageParser::default().parse(&data) {
                    let attachments: Vec<_> = parsed.attachments().collect();
                    if let Some(att) = attachments.get(index) {
                        let filename = att
                            .attachment_name()
                            .unwrap_or("unnamed")
                            .to_string();
                        let content_type = att
                            .content_type()
                            .map(|ct| {
                                if let Some(sub) = ct.subtype() {
                                    format!("{}/{}", ct.ctype(), sub)
                                } else {
                                    ct.ctype().to_string()
                                }
                            })
                            .unwrap_or_else(|| "application/octet-stream".into());
                        let body = att.contents().to_vec();

                        return (
                            StatusCode::OK,
                            [
                                ("content-type", content_type),
                                ("content-disposition", format!("attachment; filename=\"{filename}\"")),
                            ],
                            body,
                        );
                    }
                }
            }
        }
    }

    (
        StatusCode::NOT_FOUND,
        [("content-type", "text/plain".to_string()), ("content-disposition", String::new())],
        b"attachment not found".to_vec(),
    )
}

// ---------- admin API endpoints ----------

async fn list_domains(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(Vec::<crate::domain_store::Domain>::new());
    };
    Json(ds.list_domains().await.unwrap_or_default())
}

async fn add_domain(
    State(state): State<Arc<WebState>>,
    Json(req): Json<AddDomainRequest>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    let now = chrono::Utc::now().timestamp();
    match ds.add_domain(&req.name, now).await {
        Ok(()) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}

async fn remove_domain(
    Path(name): Path<String>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds.remove_domain(&name).await {
        Ok(true) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Ok(false) => Json(ApiResult {
            success: false,
            message: Some("domain not found".into()),
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}

async fn check_domain_handler(
    Path(name): Path<String>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref resolver) = state.resolver else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "DNS resolver not available"})),
        );
    };
    let report = crate::domain_check::check_domain(
        resolver,
        &name,
        state.dkim_selector.as_deref(),
        &state.hostname,
    )
    .await;
    (StatusCode::OK, Json(serde_json::to_value(report).unwrap()))
}

async fn list_accounts(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(Vec::<crate::domain_store::Account>::new());
    };
    Json(ds.list_accounts().await.unwrap_or_default())
}

async fn add_account(
    State(state): State<Arc<WebState>>,
    Json(req): Json<AddAccountRequest>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };

    // hash password
    let password_hash = if req.password.is_empty() {
        String::new()
    } else {
        crate::users::UserStore::hash_password(&req.password)
            .unwrap_or_else(|_| req.password.clone())
    };

    let now = chrono::Utc::now().timestamp();
    match ds.add_account(&req.address, &req.domain, &req.display_name, &password_hash, now).await {
        Ok(()) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}

async fn remove_account(
    Path(address): Path<String>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds.remove_account(&address).await {
        Ok(true) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Ok(false) => Json(ApiResult {
            success: false,
            message: Some("account not found".into()),
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}

// ---------- Aliases ----------

async fn list_aliases(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(serde_json::json!([]));
    };
    Json(serde_json::to_value(ds.list_aliases().await.unwrap_or_default()).unwrap_or_default())
}

#[derive(Deserialize)]
struct AddAliasRequest {
    source_address: String,
    target_address: String,
    domain: String,
    #[serde(default = "default_alias_type")]
    alias_type: String,
}

fn default_alias_type() -> String {
    "alias".into()
}

async fn add_alias(
    State(state): State<Arc<WebState>>,
    Json(req): Json<AddAliasRequest>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    let now = chrono::Utc::now().timestamp();
    match ds.add_alias(&req.source_address, &req.target_address, &req.domain, &req.alias_type, now).await {
        Ok(_) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}

async fn remove_alias(
    Path(id): Path<i64>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("domain store not configured".into()),
        });
    };
    match ds.remove_alias(id).await {
        Ok(true) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Ok(false) => Json(ApiResult {
            success: false,
            message: Some("alias not found".into()),
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}

// ---------- Quota ----------

#[derive(Serialize)]
struct QuotaResponse {
    address: String,
    quota_bytes: i64,
}

async fn get_quota(
    Path(address): Path<String>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "domain store not configured"}))).into_response();
    };
    match ds.get_quota(&address).await {
        Ok(Some(quota_bytes)) => Json(QuotaResponse { address, quota_bytes }).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "account not found"}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct SetQuotaRequest {
    quota_bytes: i64,
}

async fn set_quota(
    Path(address): Path<String>,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SetQuotaRequest>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult { success: false, message: Some("domain store not configured".into()) });
    };
    match ds.set_quota(&address, req.quota_bytes).await {
        Ok(true) => Json(ApiResult { success: true, message: None }),
        Ok(false) => Json(ApiResult { success: false, message: Some("account not found".into()) }),
        Err(e) => Json(ApiResult { success: false, message: Some(e.to_string()) }),
    }
}

// ---------- Sieve ----------

#[derive(Serialize)]
struct SieveResponse {
    address: String,
    script: Option<String>,
}

async fn get_sieve(
    Path(address): Path<String>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "domain store not configured"}))).into_response();
    };
    match ds.get_sieve_script(&address).await {
        Ok(script) => Json(SieveResponse { address, script }).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct SetSieveRequest {
    script: String,
}

async fn set_sieve(
    Path(address): Path<String>,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SetSieveRequest>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult { success: false, message: Some("domain store not configured".into()) });
    };
    // validate sieve script before saving
    if let Err(e) = crate::sieve::compile_sieve(&req.script) {
        return Json(ApiResult { success: false, message: Some(format!("invalid sieve script: {e}")) });
    }
    let now = chrono::Utc::now().timestamp();
    match ds.set_sieve_script(&address, &req.script, now).await {
        Ok(()) => Json(ApiResult { success: true, message: None }),
        Err(e) => Json(ApiResult { success: false, message: Some(e.to_string()) }),
    }
}

async fn delete_sieve(
    Path(address): Path<String>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult { success: false, message: Some("domain store not configured".into()) });
    };
    match ds.delete_sieve_script(&address).await {
        Ok(true) => Json(ApiResult { success: true, message: None }),
        Ok(false) => Json(ApiResult { success: false, message: Some("no sieve script found".into()) }),
        Err(e) => Json(ApiResult { success: false, message: Some(e.to_string()) }),
    }
}

// ---------- MTA-STS ----------

async fn mta_sts_policy(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let Some(ref mode) = state.mta_sts_mode else {
        return (StatusCode::NOT_FOUND, "MTA-STS not configured".to_string());
    };

    let mx_lines: Vec<String> = state.mta_sts_mx.iter().map(|mx| format!("mx: {mx}")).collect();
    let body = format!(
        "version: STSv1\nmode: {mode}\n{}\nmax_age: {}\nid: {}",
        mx_lines.join("\n"),
        state.mta_sts_max_age,
        state.mta_sts_id
    );

    (StatusCode::OK, body)
}

// ---------- mail client autodiscover ----------

async fn autodiscover_outlook(
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let hostname = &state.hostname;
    let xml = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<Autodiscover xmlns="http://schemas.microsoft.com/exchange/autodiscover/responseschema/2006">
  <Response xmlns="http://schemas.microsoft.com/exchange/autodiscover/outlook/responseschema/2006a">
    <Account>
      <AccountType>email</AccountType>
      <Action>settings</Action>
      <Protocol>
        <Type>IMAP</Type>
        <Server>{hostname}</Server>
        <Port>993</Port>
        <SSL>on</SSL>
        <SPA>off</SPA>
        <LoginName>%EMAILADDRESS%</LoginName>
      </Protocol>
      <Protocol>
        <Type>SMTP</Type>
        <Server>{hostname}</Server>
        <Port>465</Port>
        <SSL>on</SSL>
        <SPA>off</SPA>
        <LoginName>%EMAILADDRESS%</LoginName>
      </Protocol>
    </Account>
  </Response>
</Autodiscover>"#
    );
    (
        StatusCode::OK,
        [("content-type", "application/xml; charset=utf-8")],
        xml,
    )
}

async fn autoconfig_mozilla(
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let hostname = &state.hostname;
    let xml = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<clientConfig version="1.1">
  <emailProvider id="{hostname}">
    <domain>%EMAILDOMAIN%</domain>
    <incomingServer type="imap">
      <hostname>{hostname}</hostname>
      <port>993</port>
      <socketType>SSL</socketType>
      <authentication>password-cleartext</authentication>
      <username>%EMAILADDRESS%</username>
    </incomingServer>
    <outgoingServer type="smtp">
      <hostname>{hostname}</hostname>
      <port>465</port>
      <socketType>SSL</socketType>
      <authentication>password-cleartext</authentication>
      <username>%EMAILADDRESS%</username>
    </outgoingServer>
  </emailProvider>
</clientConfig>"#
    );
    (
        StatusCode::OK,
        [("content-type", "application/xml; charset=utf-8")],
        xml,
    )
}

// ---------- WebSocket ----------

async fn ws_events(
    ws: WebSocketUpgrade,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(socket: WebSocket, state: Arc<WebState>) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.event_bus.subscribe();

    // forward events to websocket
    let send_task = tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&event) {
                if sender.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    // drain incoming messages (keep-alive pongs handled by axum)
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(_)) = receiver.next().await {}
    });

    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
}
