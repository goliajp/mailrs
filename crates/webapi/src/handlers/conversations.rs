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
    let resp = match state.fast().list_conversations(&user, &req).await {
        Ok(resp) if !resp.items.is_empty() => resp,
        Ok(_empty) if state.fastcore_client.is_some() => state
            .core_client
            .list_conversations(&user, &req)
            .await
            .map_err(map_err)?,
        Ok(empty) => empty,
        Err(_) if state.fastcore_client.is_some() => state
            .core_client
            .list_conversations(&user, &req)
            .await
            .map_err(map_err)?,
        Err(e) => return Err(map_err(e)),
    };
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

/// GET /api/conversations/{thread_id} — return bare Vec<MessageWire>
/// (monolith shape, not wrapped in `{"items": [...]}`).
pub async fn get_thread_messages(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<Json<Vec<mailrs_core_api::method::message::MessageWire>>, StatusCode> {
    let resp = match state.fast().list_thread_messages(&user, &thread_id).await {
        Ok(resp) if !resp.items.is_empty() => resp,
        Ok(_empty) if state.fastcore_client.is_some() => state
            .core_client
            .list_thread_messages(&user, &thread_id)
            .await
            .map_err(map_err)?,
        Ok(empty) => empty,
        Err(_) if state.fastcore_client.is_some() => state
            .core_client
            .list_thread_messages(&user, &thread_id)
            .await
            .map_err(map_err)?,
        Err(e) => return Err(map_err(e)),
    };
    Ok(Json(resp.items))
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
