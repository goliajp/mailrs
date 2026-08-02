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

// Split by subject on 2026-08-02. Re-exported so the router keeps
// naming `handlers::conversations::…`.
pub use crate::handlers::conversation_body::*;
pub use crate::handlers::conversation_verbs::*;

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
    pub received_count: u32,
    pub sent_count: u32,
}

impl From<mailrs_core_api::types::ConversationSummaryWire> for ConversationResponse {
    fn from(w: mailrs_core_api::types::ConversationSummaryWire) -> Self {
        let participants: Vec<String> = w
            .participants
            .split(',')
            .map(|s| mailrs_rfc2047::decode(s.trim().as_bytes()).into_owned())
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
            // idempotent rfc2047 pass: rows ingested before the from/to
            // decode fix (fastcore lib.rs) still hold =?..?= runes
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
        .core
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
        .core
        .conversation_categories(&user)
        .await
        .map(|r| Json(r.categories))
        .map_err(map_err)
}

/// GET /api/mail/sent — one row per outbound message (not per thread),
/// newest first, each carrying the recipient (To) + thread_id + uid so
/// the Sent view lists every sent message and a click focuses it.
pub async fn list_sent_messages(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<Vec<mailrs_core_api::method::thread::SentMessageSummary>>, StatusCode> {
    let resp = state
        .core
        .list_sent_messages(&user)
        .await
        .map_err(map_err)?;
    Ok(Json(resp.items))
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
        .core
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

/// GET /api/conversations/unseen-count — returns `{"count": N}` inline
/// (monolith shape).
pub async fn get_unseen_count(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state
        .core
        .unseen_count(&user)
        .await
        .map(|r| Json(serde_json::json!({ "count": r.count })))
        .map_err(map_err)
}

pub(crate) fn map_err(e: mailrs_core_api::error::CoreApiError) -> StatusCode {
    let code = e.status_code();
    StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

#[cfg(test)]
mod cc_tests {
    use super::extract_cc_header;

    #[test]
    fn returns_none_when_no_cc_header() {
        let m = b"From: a@x\r\nTo: b@x\r\nSubject: hi\r\n\r\nbody";
        assert_eq!(extract_cc_header(m), None);
    }

    #[test]
    fn extracts_single_line_cc() {
        let m = b"From: a@x\r\nTo: b@x\r\nCc: c@x, d@x\r\nSubject: hi\r\n\r\nbody";
        assert_eq!(extract_cc_header(m).as_deref(), Some("c@x, d@x"));
    }

    #[test]
    fn extracts_folded_cc() {
        let m = b"From: a@x\r\nCc: c@x,\r\n d@x,\r\n\te@x\r\nSubject: hi\r\n\r\nbody";
        assert_eq!(extract_cc_header(m).as_deref(), Some("c@x, d@x, e@x"));
    }

    #[test]
    fn case_insensitive_header_name() {
        let m = b"From: a@x\r\nCC: c@x\r\n\r\nbody";
        assert_eq!(extract_cc_header(m).as_deref(), Some("c@x"));
    }

    #[test]
    fn stops_at_header_terminator() {
        let m = b"From: a@x\r\n\r\nCc: fake@x\r\nbody";
        assert_eq!(extract_cc_header(m), None);
    }

    #[test]
    fn empty_cc_returns_none() {
        let m = b"From: a@x\r\nCc:   \r\n\r\nbody";
        assert_eq!(extract_cc_header(m), None);
    }
}
