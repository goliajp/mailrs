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

/// Request body for `POST /v1/users/{user}/conversations:search`.
///
/// Served by the kevy text index since 2026-07-20; the Meili sentence
/// this used to carry described a service that no longer exists.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SearchConversationsRequest {
    /// Free-text query string.
    pub query: String,
    /// The list the search was issued from, in the type the list itself
    /// is asked with, so the two cannot be told different things.
    ///
    /// Search used to take five of these axes and drop the rest, so a
    /// search typed into Pinned, Snoozed, the Trash or a label answered
    /// from every list. Unless the query names a place with `in:`, a
    /// result is a conversation that list shows. `limit` caps the
    /// results (0 is [`DEFAULT_SEARCH_LIMIT`]); search answers in one
    /// page, so `before_ts` and `after_cursor` are not read.
    #[serde(flatten)]
    pub filter: ConversationFilter,
    /// Search the conversations held for review: the Review list, which
    /// is its own route rather than an axis of the filter.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub quarantined: bool,
}

/// How many results a search returns when the request leaves it open.
pub const DEFAULT_SEARCH_LIMIT: usize = 50;

/// How many conversations of a list of its own a search reads.
///
/// Such a list is read whole to scope the search, so a conversation past
/// this many in it is not found from it.
pub const LIST_SCOPE_CAP: usize = 10_000;

impl SearchConversationsRequest {
    /// The most results to return.
    pub fn limit(&self) -> usize {
        match self.filter.limit {
            0 => DEFAULT_SEARCH_LIMIT,
            n => n as usize,
        }
    }

    /// Whether the list searched is one read off an index of its own —
    /// Review, a label, Pinned, Snoozed, the Trash, Assigned to me or a
    /// section — rather than a folder's list.
    ///
    /// Such a list is usually a small part of the mailbox, so the best
    /// text matches of the whole mailbox can all fall outside it.
    pub fn reads_a_list_of_its_own(&self) -> bool {
        let f = &self.filter;
        self.quarantined
            || f.label.is_some()
            || f.pinned == Some(true)
            || f.snoozed == Some(true)
            || f.trashed
            || f.assigned_to_me
            || f.section.is_some()
    }

    /// How many text matches to ask the index for before the list's scope
    /// drops some: eight pages for a folder's list, which most matches
    /// are in, and as many as the scope reads for a list of its own.
    pub fn text_hits_wanted(&self) -> usize {
        match self.reads_a_list_of_its_own() {
            true => LIST_SCOPE_CAP,
            false => self.limit().saturating_mul(8),
        }
    }
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
    /// Number of unread, non-archived, non-spam threads not held for review
    /// (excluding self-replies).
    pub count: i64,
    /// Of those, the ones the Inbox lists.
    #[serde(default)]
    pub inbox: i64,
    /// And the ones Notifications & Promotions lists; `inbox + np == count`.
    #[serde(default)]
    pub np: i64,
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
        assert_eq!(req.limit(), 50);
    }

    /// A body written before the list's axes were added asks the same
    /// question it always did.
    #[test]
    fn an_old_search_body_reads_as_the_same_scope() {
        let s = r#"{"query":"hi","category":"bulk","limit":7,"folder":"Inbox","unread":true,"starred":true,"archived":true}"#;
        let req: SearchConversationsRequest = serde_json::from_str(s).unwrap();
        assert_eq!(req.limit(), 7);
        assert_eq!(req.filter.folder.as_deref(), Some("Inbox"));
        assert_eq!(req.filter.category.as_deref(), Some("bulk"));
        assert_eq!(req.filter.unread, Some(true));
        assert_eq!(req.filter.starred, Some(true));
        assert!(req.filter.archived);
        assert!(!req.filter.trashed && !req.filter.assigned_to_me && !req.quarantined);
        assert!(req.filter.label.is_none() && req.filter.section.is_none());
        assert!(req.filter.pinned.is_none() && req.filter.snoozed.is_none());
    }

    #[test]
    fn the_list_axes_travel_at_the_top_level() {
        let req = SearchConversationsRequest {
            query: "hi".into(),
            filter: ConversationFilter {
                pinned: Some(true),
                label: Some("3".into()),
                section: Some("awaiting".into()),
                assigned_to_me: true,
                trashed: true,
                ..Default::default()
            },
            quarantined: false,
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["pinned"], true);
        assert_eq!(v["label"], "3");
        assert_eq!(v["section"], "awaiting");
        assert_eq!(v["assigned_to_me"], true);
        assert_eq!(v["trashed"], true);
        assert!(v.get("quarantined").is_none());
        let back: SearchConversationsRequest = serde_json::from_value(v).unwrap();
        assert_eq!(back.filter.label.as_deref(), Some("3"));
        assert!(back.filter.assigned_to_me && back.filter.trashed);
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
