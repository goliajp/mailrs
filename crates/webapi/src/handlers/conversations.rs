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

pub async fn get_conversations(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Query(q): Query<ListQuery>,
) -> Result<Json<wire::ListConversationsResponse>, StatusCode> {
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
    // Fastcore-first with graceful fallback to monolith core: during
    // the migration cutover fastcore's kevy may not have every thread
    // yet, so a 5xx or empty response silently degrades to core so the
    // user still sees their inbox. Once catchup is caught up, this
    // fallback path never trips.
    match state.fast().list_conversations(&user, &req).await {
        Ok(resp) if !resp.items.is_empty() => Ok(Json(resp)),
        Ok(_empty) if state.fastcore_client.is_some() => state
            .core_client
            .list_conversations(&user, &req)
            .await
            .map(Json)
            .map_err(map_err),
        Ok(empty) => Ok(Json(empty)),
        Err(_) if state.fastcore_client.is_some() => state
            .core_client
            .list_conversations(&user, &req)
            .await
            .map(Json)
            .map_err(map_err),
        Err(e) => Err(map_err(e)),
    }
}

/// GET /api/conversations/categories
pub async fn get_categories(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<wire::ConversationCategoriesResponse>, StatusCode> {
    state
        .fast()
        .conversation_categories(&user)
        .await
        .map(Json)
        .map_err(map_err)
}

/// GET /api/conversations/action-count
pub async fn get_action_count(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<wire::ActionCountResponse>, StatusCode> {
    state
        .fast()
        .action_count(&user)
        .await
        .map(Json)
        .map_err(map_err)
}

/// GET /api/conversations/{thread_id}
pub async fn get_thread_messages(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<Json<mailrs_core_api::method::thread::ListThreadMessagesResponse>, StatusCode> {
    state
        .fast()
        .list_thread_messages(&user, &thread_id)
        .await
        .map(Json)
        .map_err(map_err)
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

/// GET /api/conversations/unseen-count
pub async fn get_unseen_count(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<wire::UnseenCountResponse>, StatusCode> {
    state
        .fast()
        .unseen_count(&user)
        .await
        .map(Json)
        .map_err(map_err)
}

fn map_err(e: mailrs_core_api::error::CoreApiError) -> StatusCode {
    let code = e.status_code();
    StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}
