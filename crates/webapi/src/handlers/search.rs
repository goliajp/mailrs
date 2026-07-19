//! `/api/conversations/semantic-search` — ranked lookup over the
//! caller's conversations.
//!
//! Served by the kevy full-text index (`KIND text` over the thread
//! rows' synthesised `search_blob` field). kevy maintains that index
//! from its commit hook, so it cannot fall behind the rows — which is
//! the failure mode that killed the previous design. Ranking is BM25
//! and CJK works without an analyzer.
//!
//! A linear scan remains only for the case where the index query
//! itself fails; an empty index result is returned as-is, because the
//! index is maintained with the writes and "no hits" is a real answer.
//!
//! This endpoint returned `[]` for every query on prod for weeks
//! (reported by goliajp 2026-07-19) — two independent defects, either
//! sufficient on its own:
//!
//!   1. it queried a Meili index named `conversations-<user>` while the
//!      writer populated `mailrs_<user>`. Meili 404'd every request and
//!      the error was swallowed, so the endpoint permanently reported
//!      the fallback backend. Three separate files each carried their
//!      own copy of that naming rule, and two disagreed.
//!   2. the fallback read thread rows out of the **network** kevy, but
//!      conversations live in fastcore's embedded store — the zrange
//!      returned nothing, the loop body never ran, and `[]` was the
//!      deterministic answer.
//!
//! Both are structurally gone now: the index lives in the same store as
//! the rows, and reads go through the core RPC like every other
//! conversation read.
//!
//! A backend that is down answers 503 rather than an empty 200 — the
//! reporter could not tell "no matches" from "search is broken", which
//! is why this went unnoticed for weeks.

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
        // The index is maintained with the writes, so an empty result is
        // the answer, not a symptom. Returning it directly also avoids
        // dragging 20k rows through the fallback on every query that
        // legitimately matches nothing.
        Ok(resp) => {
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
        // Only a genuine failure falls through — and it is logged at
        // warn, because a silent downgrade is how the previous design
        // stayed broken for weeks.
        Err(e) => {
            tracing::warn!(err = %e, %user, "kevy text search failed, falling back to scan");
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
            // Same three fields the text index covers. Substring
            // matching carries CJK without a tokenizer, which matters
            // here — most of this mail is Japanese.
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
