//! Conversation aggregate endpoints (the cascade-prone family).
//!
//! Sources:
//! - `crates/mailbox/src/pg/thread_ops/query.rs:18`  — `list_conversations` (Rock 1)
//! - `crates/mailbox/src/pg/thread_ops/query.rs:240` — `get_conversations_by_thread_ids`
//! - `crates/mailbox/src/pg/search_ops.rs:7`         — `search_conversations` (Rock 4)
//! - `crates/mailbox/src/pg/search_ops.rs:176`       — `list_conversation_categories`
//! - `crates/mailbox/src/pg/message_ops/read.rs:203` — `count_unseen` (Rock 2)
//!
//! These are the highest-value endpoints for the fastcore design — Rock 1
//! cascade today, KV-feasible by precomputing thread hashes on write.

use serde::{Deserialize, Serialize};

use crate::types::{ConversationFilter, ConversationSummaryWire, ThreadId};

// ── paths ───────────────────────────────────────────────────────────

pub const PATH_LIST_CONVERSATIONS: &str = "/v1/users/{user}/conversations:list";
pub const PATH_CONVERSATIONS_BY_THREAD_IDS: &str = "/v1/users/{user}/conversations:by-thread-ids";
pub const PATH_SEARCH_CONVERSATIONS: &str = "/v1/users/{user}/conversations:search";
pub const PATH_CONVERSATION_CATEGORIES: &str = "/v1/users/{user}/conversations/categories";
pub const PATH_UNSEEN_COUNT: &str = "/v1/users/{user}/conversations/unseen-count";

// ── list_conversations (Rock 1) ─────────────────────────────────────

/// Request body for `POST /v1/users/{user}/conversations:list`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListConversationsRequest {
    /// Filter axes — full surface of the 10-arg `list_conversations` SQL.
    #[serde(flatten)]
    pub filter: ConversationFilter,
}

/// Response body for `POST /v1/users/{user}/conversations:list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListConversationsResponse {
    /// Conversation summaries, sorted by pinned DESC then last_date DESC.
    pub items: Vec<ConversationSummaryWire>,
}

// ── get_conversations_by_thread_ids ─────────────────────────────────

/// Request body for `POST /v1/users/{user}/conversations:by-thread-ids`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationsByIdsRequest {
    /// Thread IDs to hydrate (typically from a search result).
    pub thread_ids: Vec<ThreadId>,
    /// Optional folder filter applied during hydration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
}

/// Response body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationsByIdsResponse {
    /// Conversation summaries in the requested order.
    pub items: Vec<ConversationSummaryWire>,
}

// ── search_conversations (Rock 4) ───────────────────────────────────

/// Request body for `GET /v1/users/{user}/conversations:search?q=...`.
///
/// Meili is the primary backend; this RPC is the PG FTS fallback.
/// `fastcore` returns `BackendUnsupported` for this endpoint (webapi must
/// degrade gracefully by relying solely on meili).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConversationsRequest {
    /// Free-text query string.
    pub query: String,
    /// Optional category filter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Max items to return.
    #[serde(default = "default_search_limit")]
    pub limit: u32,
}

fn default_search_limit() -> u32 {
    50
}

/// Response body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConversationsResponse {
    /// Conversation summaries (subset of fields) matching the query.
    pub items: Vec<ConversationSummaryWire>,
}

// ── list_conversation_categories ────────────────────────────────────

/// Response body for `GET /v1/users/{user}/conversations/categories`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationCategoriesResponse {
    /// Category name → number of distinct thread_ids in it.
    pub categories: Vec<CategoryCount>,
}

/// One row in the categories histogram.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CategoryCount {
    /// Category label (`personal`, `bulk`, `spam`, ...).
    pub category: String,
    /// Number of distinct thread_ids classified under this category.
    pub count: i64,
}

// ── unseen_count (Rock 2) ───────────────────────────────────────────

/// Response body for `GET /v1/users/{user}/conversations/unseen-count`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UnseenCountResponse {
    /// Number of unread, non-archived, non-spam threads (excluding self-replies).
    pub count: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_request_roundtrip() {
        let req = ListConversationsRequest {
            filter: ConversationFilter {
                limit: 50,
                folder: Some("INBOX".into()),
                ..Default::default()
            },
        };
        let s = serde_json::to_string(&req).unwrap();
        // flatten — fields should appear at top level
        assert!(s.contains("\"limit\":50"));
        assert!(s.contains("\"folder\":\"INBOX\""));
        let back: ListConversationsRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(back.filter.limit, 50);
        assert_eq!(back.filter.folder.as_deref(), Some("INBOX"));
    }

    #[test]
    fn search_limit_defaults_to_50() {
        let s = r#"{"query":"hello"}"#;
        let req: SearchConversationsRequest = serde_json::from_str(s).unwrap();
        assert_eq!(req.limit, 50);
    }

    #[test]
    fn categories_response_roundtrip() {
        let resp = ConversationCategoriesResponse {
            categories: vec![
                CategoryCount {
                    category: "personal".into(),
                    count: 42,
                },
                CategoryCount {
                    category: "bulk".into(),
                    count: 7,
                },
            ],
        };
        let s = serde_json::to_string(&resp).unwrap();
        let back: ConversationCategoriesResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(back.categories.len(), 2);
        assert_eq!(back.categories[0].count, 42);
    }
}
