//! Wire types shared across RPC methods.
//!
//! Domain types in `mailrs-mailbox` lack `Serialize`/`Deserialize` derives
//! and the ironrule forbids touching that crate to add them. So this module
//! defines wire mirrors with the same field shape + Serde + `From` /
//! `to_summary` conversions in both directions.
//!
//! Conversions live here so neither `mailrs-mailbox` nor the server cement
//! has to be modified (ironrule).

use serde::{Deserialize, Serialize};

// ──────────────────────────────────────────────────────────────────────
// Health
// ──────────────────────────────────────────────────────────────────────

/// Health probe response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HealthResponse {
    /// Wire version this server speaks (matches `API_VERSION`).
    pub version: String,
    /// Which backend is running: `"pg"` (core) or `"kevy"` (fastcore).
    pub backend: BackendKind,
    /// `true` if all dependencies are reachable (backend + meili if used).
    pub ready: bool,
}

/// Which storage backend a `mailrs-core-api` server is wrapping.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    /// `mailrs-core` running on PG (today's `spg` cement).
    Pg,
    /// `mailrs-fastcore` running on kevy (experimental).
    Kevy,
}

// ──────────────────────────────────────────────────────────────────────
// Pagination
// ──────────────────────────────────────────────────────────────────────

/// Pagination envelope used by list endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListEnvelope<T> {
    /// Items in this page.
    pub items: Vec<T>,
    /// Opaque cursor to pass to the next request (None = end).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

// ──────────────────────────────────────────────────────────────────────
// Identifier aliases
// ──────────────────────────────────────────────────────────────────────

/// User scope identifier (email address). Used as the primary "tenant" key.
pub type UserAddress = String;

/// Internal numeric ID for a mailbox row.
pub type MailboxId = i64;

/// Internal numeric ID for a message row.
pub type MessageId = i64;

/// Maildir-style on-disk identifier for a message.
pub type MaildirId = String;

/// Thread identifier (string hash from References/Message-ID chain).
pub type ThreadId = String;

// ──────────────────────────────────────────────────────────────────────
// Conversation wire types (mirror `mailrs_mailbox::ConversationSummary`)
// ──────────────────────────────────────────────────────────────────────

/// Wire mirror of `mailrs_mailbox::ConversationSummary`.
///
/// Same field shape, with Serde derives. Conversion via `From` impls.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConversationSummaryWire {
    pub thread_id: ThreadId,
    pub subject: String,
    pub participants: String,
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
    pub sent_count: u32,
}

impl From<&mailrs_mailbox::types::ConversationSummary> for ConversationSummaryWire {
    fn from(s: &mailrs_mailbox::types::ConversationSummary) -> Self {
        Self {
            thread_id: s.thread_id.clone(),
            subject: s.subject.clone(),
            participants: s.participants.clone(),
            message_count: s.message_count,
            unread_count: s.unread_count,
            last_date: s.last_date,
            category: s.category.clone(),
            flagged: s.flagged,
            snippet: s.snippet.clone(),
            pinned: s.pinned,
            archived: s.archived,
            importance_level: s.importance_level.clone(),
            importance_score: s.importance_score,
            requires_action: s.requires_action,
            sent_count: s.sent_count,
        }
    }
}

impl From<mailrs_mailbox::types::ConversationSummary> for ConversationSummaryWire {
    fn from(s: mailrs_mailbox::types::ConversationSummary) -> Self {
        (&s).into()
    }
}

// ──────────────────────────────────────────────────────────────────────
// Conversation filter (matches the 10 args of `list_conversations`)
// ──────────────────────────────────────────────────────────────────────

/// Filter axes for `list_conversations`.
///
/// Mirrors the 10-arg signature in
/// `crates/mailbox/src/pg/thread_ops/query.rs:18`. All filters are
/// optional except `limit`; `user` is implicit in the path.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConversationFilter {
    /// Max items to return.
    pub limit: u32,
    /// Page cursor: epoch-seconds, returns threads with `last_date < before_ts`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_ts: Option<i64>,
    /// Limit to a single category (`personal` / `bulk` / `spam` / `scam` / ...).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// When set, query across these domains instead of single `user`.
    /// Empty Vec = single-user mode (alias for None).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domains: Option<Vec<String>>,
    /// `true` shows only archived threads, `false` (default) hides them.
    #[serde(default)]
    pub archived: bool,
    /// Restrict to a single mailbox name (e.g. `INBOX`, `Sent`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// `Some(true)` = unread only, `Some(false)` = read only, `None` = both.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unread: Option<bool>,
    /// `Some(true)` = starred only, etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starred: Option<bool>,
    /// One of: `important` / `other` / `null` (UI section tabs).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
}

// ──────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_kind_serde() {
        let pg = BackendKind::Pg;
        let s = serde_json::to_string(&pg).unwrap();
        assert_eq!(s, "\"pg\"");
        let back: BackendKind = serde_json::from_str(&s).unwrap();
        assert_eq!(back, BackendKind::Pg);

        let s2 = serde_json::to_string(&BackendKind::Kevy).unwrap();
        assert_eq!(s2, "\"kevy\"");
    }

    #[test]
    fn list_envelope_omits_cursor_when_none() {
        let env = ListEnvelope::<i32> {
            items: vec![1, 2, 3],
            next_cursor: None,
        };
        let s = serde_json::to_string(&env).unwrap();
        assert!(!s.contains("next_cursor"));
    }

    #[test]
    fn conversation_filter_omits_empty_options() {
        let f = ConversationFilter {
            limit: 50,
            ..Default::default()
        };
        let s = serde_json::to_string(&f).unwrap();
        assert!(!s.contains("category"));
        assert!(!s.contains("domains"));
        assert!(!s.contains("folder"));
        assert!(s.contains("\"limit\":50"));
        assert!(s.contains("\"archived\":false"));
    }

    #[test]
    fn conversation_summary_wire_roundtrip() {
        let w = ConversationSummaryWire {
            thread_id: "tid-1".into(),
            subject: "hello".into(),
            participants: "a@x.com,b@y.com".into(),
            message_count: 3,
            unread_count: 1,
            last_date: 1_700_000_000,
            category: "personal".into(),
            flagged: true,
            snippet: "preview".into(),
            pinned: false,
            archived: false,
            importance_level: "important".into(),
            importance_score: 0.8,
            requires_action: true,
            sent_count: 0,
        };
        let s = serde_json::to_string(&w).unwrap();
        let back: ConversationSummaryWire = serde_json::from_str(&s).unwrap();
        assert_eq!(w, back);
    }

    #[test]
    fn conversation_filter_full_roundtrip() {
        let f = ConversationFilter {
            limit: 50,
            before_ts: Some(1_700_000_000),
            category: Some("personal".into()),
            domains: Some(vec!["example.com".into(), "test.com".into()]),
            archived: true,
            folder: Some("INBOX".into()),
            unread: Some(true),
            starred: Some(false),
            section: Some("important".into()),
        };
        let s = serde_json::to_string(&f).unwrap();
        let back: ConversationFilter = serde_json::from_str(&s).unwrap();
        assert_eq!(back.limit, f.limit);
        assert_eq!(back.before_ts, f.before_ts);
        assert_eq!(back.domains.as_ref().map(|v| v.len()), Some(2));
    }
}
