//! Miscellaneous fastcore-native handlers — keys, deliverability
//! check, spam-feedback, search.

use std::sync::Arc;

use crate::handlers::kevy_util::with_kevy;
use axum::{
    Json,
    extract::{Extension, Query, State},
    http::StatusCode,
};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

fn map_core_err(e: mailrs_core_api::error::CoreApiError) -> StatusCode {
    StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

// ── PGP keys (network kevy hash) ──────────────────────────────────
//
// Keys:
//   pgp_keys:<user>       hash  { <email> -> <ascii-armored public key> }

/// GET /api/mail/keys — list saved public keys for the current user.
pub async fn get_keys(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = format!("pgp_keys:{user}");
    let flat = with_kevy(move |c| c.hgetall(key.as_bytes()).map_err(std::io::Error::from))?;
    let mut items = Vec::new();
    let mut i = 0;
    while i + 1 < flat.len() {
        items.push(serde_json::json!({
            "email": String::from_utf8_lossy(&flat[i]),
            "key_armored": String::from_utf8_lossy(&flat[i + 1]),
        }));
        i += 2;
    }
    Ok(Json(serde_json::json!({ "items": items })))
}

#[derive(Debug, serde::Deserialize)]
pub struct SaveKeyRequest {
    pub email: String,
    pub key_armored: String,
}

/// POST /api/mail/keys — upsert a key entry.
pub async fn save_key(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<SaveKeyRequest>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("pgp_keys:{user}");
    let email = req.email;
    let body = req.key_armored;
    with_kevy(move |c| {
        c.hset(key.as_bytes(), &[(email.as_bytes(), body.as_bytes())])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Spam feedback (network kevy hash) ─────────────────────────────

#[derive(Debug, serde::Deserialize)]
pub struct SpamFeedbackRequest {
    pub message_id: String,
    pub is_spam: bool,
}

/// POST /api/mail/spam-feedback — record the user's spam vote.
/// Persisted to `spam_feedback:<user>` hash for eventual bulk export
/// to whatever trainer we wire up later.
pub async fn spam_feedback(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<SpamFeedbackRequest>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("spam_feedback:{user}");
    let mid = req.message_id;
    let val = if req.is_spam { "spam" } else { "ham" };
    with_kevy(move |c| {
        c.hset(key.as_bytes(), &[(mid.as_bytes(), val.as_bytes())])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Search ────────────────────────────────────────────────────────

#[derive(Debug, serde::Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default = "default_search_limit")]
    pub limit: u32,
    /// The tab the search was issued from. Absent = every folder,
    /// which is what this endpoint used to do unconditionally.
    pub folder: Option<String>,
    pub category: Option<String>,
    pub unread: Option<bool>,
    pub starred: Option<bool>,
    #[serde(default)]
    pub archived: bool,
}

fn default_search_limit() -> u32 {
    50
}

/// GET /api/conversations/search?q=&limit= — full-text search across
/// the user's threads.
///
/// Ranked by the kevy text index that fastcore maintains inside the
/// same store as the rows, then hydrated through the by-thread-ids RPC
/// so the UI gets unread / flags / category like a normal list. This
/// used to query a Meilisearch sidecar, which was removed: it was a
/// second copy of the index that could drift from the rows, and did —
/// silently, for weeks (2026-07-19).
pub async fn search_conversations(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Vec<crate::handlers::conversations::ConversationResponse>>, StatusCode> {
    let req = mailrs_core_api::method::conversation::SearchConversationsRequest {
        query: q.q,
        category: q.category,
        limit: q.limit,
        folder: q.folder,
        unread: q.unread,
        starred: q.starred,
        archived: q.archived,
    };
    let hits = state
        .core
        .search_conversations(&user, &req)
        .await
        .map_err(map_core_err)?;
    let thread_ids: Vec<String> = hits.items.iter().map(|c| c.thread_id.clone()).collect();
    if thread_ids.is_empty() {
        return Ok(Json(Vec::new()));
    }
    // hydrate full rows, preserving relevance order
    let hydrate = mailrs_core_api::method::conversation::ConversationsByIdsRequest {
        thread_ids,
        folder: None,
    };
    let rows = state
        .core
        .conversations_by_thread_ids(&user, &hydrate)
        .await
        .map_err(map_core_err)?;
    let out: Vec<crate::handlers::conversations::ConversationResponse> =
        rows.items.into_iter().map(Into::into).collect();
    Ok(Json(out))
}
