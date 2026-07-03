//! Handlers for `mailrs_core_api::method::conversation`.
//!
//! Six endpoints — all `Arc<PgMailboxStore>.method(...)` passes through.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use mailrs_core_api::method::conversation as wire;
use mailrs_core_api::types::ConversationSummaryWire;

use crate::core_rpc::CoreRpcState;

/// POST /v1/users/{user}/conversations:list  (Rock 1)
pub async fn list_conversations(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Json(req): Json<wire::ListConversationsRequest>,
) -> Result<Json<wire::ListConversationsResponse>, StatusCode> {
    let f = &req.filter;
    let category = f.category.as_deref();
    let folder = f.folder.as_deref();
    let section = f.section.as_deref();
    let domains = f.domains.as_deref();

    let rows = state
        .mailbox
        .list_conversations(
            &user,
            f.limit,
            f.before_ts,
            category,
            domains,
            f.archived,
            folder,
            f.unread,
            f.starred,
            section,
        )
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, "list_conversations failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let items: Vec<ConversationSummaryWire> = rows.iter().map(Into::into).collect();
    Ok(Json(wire::ListConversationsResponse { items }))
}

/// POST /v1/users/{user}/conversations:by-thread-ids
pub async fn conversations_by_thread_ids(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Json(req): Json<wire::ConversationsByIdsRequest>,
) -> Result<Json<wire::ConversationsByIdsResponse>, StatusCode> {
    let rows = state
        .mailbox
        .get_conversations_by_thread_ids(&user, &req.thread_ids, None)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, "by_thread_ids failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let items: Vec<ConversationSummaryWire> = rows.iter().map(Into::into).collect();
    Ok(Json(wire::ConversationsByIdsResponse { items }))
}

/// GET /v1/users/{user}/conversations/categories
pub async fn conversation_categories(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
) -> Result<Json<wire::ConversationCategoriesResponse>, StatusCode> {
    let rows = state
        .mailbox
        .list_conversation_categories(&user, None)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, "list_categories failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let categories = rows
        .into_iter()
        .map(|(category, count)| wire::CategoryCount { category, count })
        .collect();
    Ok(Json(wire::ConversationCategoriesResponse { categories }))
}

/// GET /v1/users/{user}/conversations/action-count  (Rock 2)
pub async fn action_count(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
) -> Result<Json<wire::ActionCountResponse>, StatusCode> {
    let count = state
        .mailbox
        .count_action_threads(&user, None)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, "action_count failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(wire::ActionCountResponse { count }))
}

/// GET /v1/users/{user}/conversations/unseen-count  (Rock 2)
pub async fn unseen_count(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
) -> Result<Json<wire::UnseenCountResponse>, StatusCode> {
    let count = state.mailbox.count_unseen(&user).await.map_err(|e| {
        tracing::warn!(error = %e, user = %user, "unseen_count failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(wire::UnseenCountResponse { count }))
}
