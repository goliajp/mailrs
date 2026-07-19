//! `/api/conversations/semantic-search` — ranked lookup over the
//! caller's conversations.
//!
//! Meili serves it when reachable and the index answers; otherwise a
//! scan of the caller's conversations does. Both paths match subject,
//! participants and preview, so the two backends agree on what counts
//! as a hit even though they disagree on ranking.
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

    if let Some(hits) = try_meili(&user, &q.q, limit).await {
        return Ok(Json(
            serde_json::json!({ "items": hits, "backend": "meili" }),
        ));
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

/// Query Meili. `None` on any failure so the caller falls back — an
/// unreachable or not-yet-populated index must not fail the request
/// outright while a working scan exists.
async fn try_meili(user: &str, query: &str, limit: u32) -> Option<Vec<serde_json::Value>> {
    let base = std::env::var("MAILRS_MEILI_URL").ok()?;
    let index = mailrs_core_api::meili_index_name(user);
    let url = format!("{base}/indexes/{index}/search");
    let mut req = reqwest::Client::new().post(&url).json(&serde_json::json!({
        "q": query,
        "limit": limit,
        "attributesToRetrieve": ["thread_id", "subject", "participants"],
    }));
    if let Ok(k) = std::env::var("MAILRS_MEILI_KEY") {
        req = req.bearer_auth(k);
    }
    let resp = req.send().await.ok()?;
    if !resp.status().is_success() {
        tracing::debug!(status = %resp.status(), %index, "meili search rejected");
        return None;
    }
    let v = resp.json::<serde_json::Value>().await.ok()?;
    let hits = v.get("hits")?.as_array()?.clone();
    Some(hits)
}
