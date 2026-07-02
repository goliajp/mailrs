//! `/api/conversations*` REST handlers — thin shims that delegate to the
//! core RPC client.
//!
//! Phase 3.5 — replaces the monolith's direct `state.mailbox_store.X()`
//! calls (REST agent inventory in `docs/CURRENT_STATE_FROZEN.md` §0.2)
//! with `state.core_client.X()` RPC calls.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
};
use mailrs_core_api::method::conversation as wire;
use mailrs_core_api::types::ConversationFilter;
use serde::Deserialize;

use crate::WebState;

/// Resolved user identity carried via axum Extension by the auth layer
/// (set by `session::session_auth_middleware`).
#[derive(Debug, Clone)]
pub struct AuthedUser(pub String);

/// Optional display name from the session blob — set by the auth layer
/// when available, blank when the dev fallback header path is used.
#[derive(Debug, Clone, Default)]
pub struct AuthedDisplayName(pub String);

/// GET /api/conversations  — query-string filter, returns the list.
#[derive(Debug, Deserialize, Default)]
pub struct ListQuery {
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default, alias = "before")]
    pub before_ts: Option<i64>,
    pub category: Option<String>,
    pub folder: Option<String>,
    #[serde(default)]
    pub archived: bool,
    pub unread: Option<bool>,
    pub starred: Option<bool>,
    pub section: Option<String>,
}

fn default_limit() -> u32 {
    50
}

/// Wire shape the React UI expects for /api/conversations.
///
/// Same as monolith's `ConversationResponse` — critical difference from
/// fastcore's `ConversationSummaryWire` is `participants` is a `Vec<String>`
/// (split by comma) instead of the raw csv string. UI does
/// `convo.participants[0]` which on a plain string returns the first
/// CHARACTER, not the first sender.
#[derive(serde::Serialize)]
pub struct ConversationResponse {
    pub thread_id: String,
    pub subject: String,
    pub participants: Vec<String>,
    pub message_count: u32,
    pub unread_count: u32,
    pub last_date: i64,
    pub category: String,
    pub flagged: bool,
    pub snippet: String,
    pub pinned: bool,
    pub archived: bool,
    pub importance_level: String,
    pub importance_score: f32,
    pub requires_action: bool,
    pub last_sender: String,
    pub received_count: u32,
    pub sent_count: u32,
}

impl From<mailrs_core_api::types::ConversationSummaryWire> for ConversationResponse {
    fn from(w: mailrs_core_api::types::ConversationSummaryWire) -> Self {
        let participants: Vec<String> = w
            .participants
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let received_count = w.message_count.saturating_sub(w.sent_count);
        Self {
            thread_id: w.thread_id,
            subject: w.subject,
            participants,
            message_count: w.message_count,
            unread_count: w.unread_count,
            last_date: w.last_date,
            category: w.category,
            flagged: w.flagged,
            snippet: w.snippet,
            pinned: w.pinned,
            archived: w.archived,
            importance_level: w.importance_level,
            importance_score: w.importance_score,
            requires_action: w.requires_action,
            last_sender: w.last_sender,
            received_count,
            sent_count: w.sent_count,
        }
    }
}

pub async fn get_conversations(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<ConversationResponse>>, StatusCode> {
    let req = wire::ListConversationsRequest {
        filter: ConversationFilter {
            limit: q.limit,
            before_ts: q.before_ts,
            category: q.category,
            domains: None,
            archived: q.archived,
            folder: q.folder,
            unread: q.unread,
            starred: q.starred,
            section: q.section,
        },
    };
    let resp = state
        .fast()
        .list_conversations(&user, &req)
        .await
        .map_err(map_err)?;
    Ok(Json(resp.items.into_iter().map(Into::into).collect()))
}

/// GET /api/conversations/categories — return bare Vec<CategoryCount>
/// (monolith shape, not wrapped in `{"categories": [...]}`).
pub async fn get_categories(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<Vec<wire::CategoryCount>>, StatusCode> {
    state
        .fast()
        .conversation_categories(&user)
        .await
        .map(|r| Json(r.categories))
        .map_err(map_err)
}

/// GET /api/conversations/action-count — return bare `{count: N}`
/// (already the response shape, but as flat i64 not the response struct).
pub async fn get_action_count(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state
        .fast()
        .action_count(&user)
        .await
        .map(|r| Json(serde_json::json!({ "count": r.count })))
        .map_err(map_err)
}

/// Wire shape UI expects for a message. Mirrors monolith's
/// `ThreadMessageResponse` — critical fields the UI reaches into
/// unconditionally (`attachments.length`, `text_body`, `html_body`,
/// `people`, `dates`, ...). Return default empty arrays / null when
/// the source `MessageWire` lacks the analysis fields.
#[derive(serde::Serialize)]
pub struct ThreadMessageResponse {
    pub id: i64,
    pub uid: u32,
    pub sender: String,
    pub recipients: String,
    pub subject: String,
    pub flags: u32,
    pub internal_date: i64,
    pub message_id: String,
    pub text_body: Option<String>,
    pub html_body: Option<String>,
    pub attachments: Vec<serde_json::Value>,
    pub category: String,
    pub risk_score: u8,
    pub risk_reason: String,
    pub summary: String,
    pub people: serde_json::Value,
    pub dates: serde_json::Value,
    pub amounts: serde_json::Value,
    pub action_items: serde_json::Value,
    pub ai_analyzed: bool,
    pub clean_text: Option<String>,
    pub new_content: Option<String>,
    pub importance_level: String,
    pub importance_score: f32,
    pub is_bulk_sender: bool,
    pub has_tracking_pixel: bool,
    pub requires_action: bool,
    pub sender_intent: String,
    pub action_deadline: Option<String>,
}

impl ThreadMessageResponse {
    fn from_wire_no_body(w: mailrs_core_api::method::message::MessageWire) -> Self {
        Self {
            id: w.id,
            uid: w.uid,
            sender: w.sender,
            recipients: w.recipients,
            subject: w.subject,
            flags: w.flags,
            internal_date: w.internal_date,
            message_id: w.message_id,
            text_body: None,
            html_body: None,
            attachments: Vec::new(),
            category: String::from("inbox"),
            risk_score: 0,
            risk_reason: String::new(),
            summary: String::new(),
            people: serde_json::json!([]),
            dates: serde_json::json!([]),
            amounts: serde_json::json!([]),
            action_items: serde_json::json!([]),
            ai_analyzed: false,
            clean_text: None,
            new_content: None,
            importance_level: String::from("normal"),
            importance_score: 0.0,
            is_bulk_sender: false,
            has_tracking_pixel: false,
            requires_action: false,
            sender_intent: String::new(),
            action_deadline: None,
        }
    }
}

/// Parse `data` (raw RFC-5322 bytes) into text/html body + attachments.
/// Same pipeline as monolith's `crate::message_util::parse_message`.
fn parse_body(data: &[u8]) -> (Option<String>, Option<String>, Vec<serde_json::Value>) {
    let root = mailrs_mime::parse(data);
    let mut text_body: Option<String> = None;
    let mut html_body: Option<String> = None;
    for part in root.walk() {
        let mt = part.content_type.mime_type();
        if text_body.is_none() && mt == "text/plain" {
            text_body = part.body_text();
        } else if html_body.is_none() && mt == "text/html" {
            html_body = part.body_text();
        }
        if text_body.is_some() && html_body.is_some() {
            break;
        }
    }
    if text_body.is_none() && html_body.is_none() && root.children.is_empty() {
        if root.content_type.type_ == "text" {
            text_body = root.body_text();
        } else {
            text_body = Some(String::from_utf8_lossy(data).into_owned());
        }
    }
    let text_body = text_body.or_else(|| {
        html_body
            .as_deref()
            .and_then(|html| html2text::from_read(html.as_bytes(), 80).ok())
    });
    let attachments: Vec<serde_json::Value> = root
        .attachments()
        .map(|att| {
            let filename = att.attachment_filename().unwrap_or("unnamed").to_string();
            let mt = att.content_type.mime_type();
            let mt = if mt.ends_with('/') || mt.starts_with('/') {
                "application/octet-stream".to_string()
            } else {
                mt
            };
            serde_json::json!({
                "filename": filename,
                "content_type": mt,
                "size": att.body.len() as u32,
            })
        })
        .collect();
    (text_body, html_body, attachments)
}

/// Enrich a MessageWire with body content read from maildir.
async fn enrich_with_body(
    store: &dyn mailrs_message_store::MessageStore,
    maildir_root: &str,
    user: &str,
    w: mailrs_core_api::method::message::MessageWire,
) -> ThreadMessageResponse {
    let mut r = ThreadMessageResponse::from_wire_no_body(w.clone());
    if let Some((local, domain)) = user.split_once('@') {
        let path = format!("{maildir_root}/{domain}/{local}");
        let id = mailrs_message_store::MessageId(w.blob_ref.clone());
        if let Ok(Some(bytes)) = store.fetch(&path, &id).await {
            let (t, h, a) = parse_body(&bytes);
            r.text_body = t;
            r.html_body = h;
            r.attachments = a;
        }
    }
    r
}

/// GET /api/conversations/{thread_id} — return Vec<ThreadMessageResponse>
/// with monolith's exact wire shape (attachments, text_body, ...) so the
/// React UI can safely reach into arrays without null-guards.
pub async fn get_thread_messages(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<Json<Vec<ThreadMessageResponse>>, StatusCode> {
    let resp = state
        .fast()
        .list_thread_messages(&user, &thread_id)
        .await
        .map_err(map_err)?;
    let maildir_root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let store = mailrs_message_store::MaildirStore;
    let mut items = Vec::with_capacity(resp.items.len());
    for w in resp.items {
        items.push(enrich_with_body(&store, &maildir_root, &user, w).await);
    }
    Ok(Json(items))
}

/// Batch mutation request/response — matches the UI's `useBatchMutation`.
#[derive(Debug, serde::Deserialize)]
pub struct BatchRequest {
    pub action: String,
    pub thread_ids: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct BatchResponse {
    pub failed: u32,
    pub message: Option<String>,
    pub processed: u32,
    pub success: bool,
}

/// POST /api/conversations/batch — apply the same mutation across many
/// threads. Fires each individually against fastcore (kevy mutations are
/// idempotent + fast, ~2 ms each). Runs sequentially; a partial failure
/// still lets the successes stick.
pub async fn batch_mutation(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<BatchRequest>,
) -> Result<Json<BatchResponse>, StatusCode> {
    let action = req.action.as_str();
    let mut processed = 0u32;
    let mut failed = 0u32;
    for tid in &req.thread_ids {
        let f = state.fast();
        let r = match action {
            "read" => f.mark_thread_read(&user, tid).await.map(|_| ()),
            "unread" => f.mark_thread_unread(&user, tid).await.map(|_| ()),
            "star" => f.star_thread(&user, tid).await.map(|_| ()),
            "unstar" => f.unstar_thread(&user, tid).await.map(|_| ()),
            "archive" => f.archive_thread(&user, tid).await.map(|_| ()),
            "unarchive" => f.unarchive_thread(&user, tid).await.map(|_| ()),
            "delete" => f.delete_thread(&user, tid).await.map(|_| ()),
            _ => Err(mailrs_core_api::error::CoreApiError::Internal(format!(
                "unknown batch action: {action}"
            ))),
        };
        match r {
            Ok(_) => processed += 1,
            Err(_) => failed += 1,
        }
    }
    Ok(Json(BatchResponse {
        failed,
        message: None,
        processed,
        success: failed == 0,
    }))
}

/// POST /api/conversations/{thread_id}/read
pub async fn mark_thread_read(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .fast()
        .mark_thread_read(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/star
pub async fn star_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .fast()
        .star_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/archive
pub async fn archive_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .fast()
        .archive_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/unread
pub async fn mark_thread_unread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .fast()
        .mark_thread_unread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/unstar
pub async fn unstar_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .fast()
        .unstar_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/pin
pub async fn pin_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .fast()
        .pin_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/unpin
pub async fn unpin_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .fast()
        .unpin_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/unarchive
pub async fn unarchive_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .fast()
        .unarchive_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/dismiss-action
pub async fn dismiss_action(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .fast()
        .dismiss_thread_action(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// DELETE /api/conversations/{thread_id}
pub async fn delete_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .fast()
        .delete_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

#[derive(Debug, serde::Deserialize)]
pub struct SnoozeBody {
    pub snoozed_until: i64,
}

/// PUT /api/conversations/{thread_id}/snooze
pub async fn snooze_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
    Json(req): Json<SnoozeBody>,
) -> Result<StatusCode, StatusCode> {
    let wire_req = mailrs_core_api::method::thread::SnoozeRequest {
        snoozed_until: req.snoozed_until,
    };
    state
        .fast()
        .snooze_thread(&user, &thread_id, &wire_req)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// DELETE /api/conversations/{thread_id}/snooze
pub async fn unsnooze_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .fast()
        .unsnooze_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// GET /api/conversations/unseen-count — returns `{"count": N}` inline
/// (monolith shape).
pub async fn get_unseen_count(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state
        .fast()
        .unseen_count(&user)
        .await
        .map(|r| Json(serde_json::json!({ "count": r.count })))
        .map_err(map_err)
}

fn map_err(e: mailrs_core_api::error::CoreApiError) -> StatusCode {
    let code = e.status_code();
    StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}
