//! `/api/conversations/semantic-search` — ranked lookup over the
//! caller's conversations.
//!
//! Served by the kevy full-text index (`KIND text` over the thread
//! rows' synthesised `search_blob` field). kevy maintains that index
//! from its commit hook, so it cannot fall behind the rows — which is
//! the failure mode that killed the previous design. Ranking is BM25
//! and CJK works without an analyzer.
//!
//! A linear scan of the caller's conversations remains as the fallback
//! for the window before the index is backfilled.
//!
//! Two independent defects made this endpoint return `[]` for every
//! query on prod for weeks (reported by goliajp 2026-07-19):
//!
//!   1. it queried index `conversations-<user>` while the writer
//!      populated `mailrs_<user>` — Meili 404'd every request and the
//!      error was swallowed, so `backend` was permanently
//!      `linear_fallback`. The name now comes from
//!      `mailrs_core_api::meili_index_name`, which both sides share.
//!   2. the fallback read thread rows out of the **network** kevy, but
//!      conversations live in fastcore's embedded store — the zrange
//!      returned nothing, the loop body never ran, and the handler
//!      answered `[]` deterministically. It now goes through the core
//!      RPC, the same path every other conversation read uses.
//!
//! A backend that is down is now a 503 rather than an empty 200: the
//! reporter could not tell "no matches" from "search is broken", which
//! is why this went unnoticed.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_limit() -> u32 {
    20
}

/// Upper bound on conversations pulled for the fallback scan. Matches
/// the MCP `search_conversations` tool so the two agree on recall.
const SCAN_CEILING: u32 = 20_000;

/// GET /api/conversations/semantic-search?q=&limit= — returns
/// `{items: [{thread_id, subject, score}], backend}`.
pub async fn semantic_search(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let limit = q.limit.clamp(1, 200);

    // Primary: the kevy text index, via the core RPC that owns the
    // embedded store.
    let search_req = mailrs_core_api::method::conversation::SearchConversationsRequest {
        query: q.q.clone(),
        category: None,
        limit,
    };
    match state.core.search_conversations(&user, &search_req).await {
        Ok(resp) if !resp.items.is_empty() => {
            let items: Vec<serde_json::Value> = resp
                .items
                .into_iter()
                .map(|c| {
                    serde_json::json!({
                        "thread_id": c.thread_id,
                        "subject": c.subject,
                        "participants": c.participants,
                        "score": 1.0,
                    })
                })
                .collect();
            return Ok(Json(
                serde_json::json!({ "items": items, "backend": "kevy_text" }),
            ));
        }
        // An empty result is not proof the index is healthy — until the
        // backfill has run, an un-indexed mailbox also answers empty.
        // Fall through to the scan, which is authoritative either way.
        Ok(_) => {}
        Err(e) => {
            tracing::debug!(err = %e, %user, "kevy text search unavailable, scanning");
        }
    }

    let req = mailrs_core_api::method::conversation::ListConversationsRequest {
        filter: mailrs_core_api::types::ConversationFilter {
            limit: SCAN_CEILING,
            before_ts: None,
            category: None,
            domains: None,
            archived: false,
            folder: None,
            unread: None,
            starred: None,
            section: None,
        },
    };
    // A core RPC failure means we genuinely cannot answer. Say so
    // instead of returning an empty list that reads as "no matches".
    let resp = state
        .core
        .list_conversations(&user, &req)
        .await
        .map_err(|e| {
            tracing::warn!(err = %e, %user, "semantic-search: list_conversations failed");
            StatusCode::SERVICE_UNAVAILABLE
        })?;

    let needle = q.q.to_lowercase();
    let items: Vec<serde_json::Value> = resp
        .items
        .into_iter()
        .filter(|c| {
            // Case-insensitive substring across the same three fields
            // Meili indexes. Substring matching carries CJK without a
            // tokenizer, which matters here — most of this mail is
            // Japanese.
            c.subject.to_lowercase().contains(&needle)
                || c.participants.to_lowercase().contains(&needle)
                || c.snippet.to_lowercase().contains(&needle)
        })
        .take(limit as usize)
        .map(|c| {
            serde_json::json!({
                "thread_id": c.thread_id,
                "subject": c.subject,
                "participants": c.participants,
                "score": 1.0,
            })
        })
        .collect();

    Ok(Json(
        serde_json::json!({ "items": items, "backend": "linear_fallback" }),
    ))
}
