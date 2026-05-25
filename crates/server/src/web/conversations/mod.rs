//! `/api/conversations*` web handlers, split by concern:
//!
//! - [`queries`] — read-only fetches.
//! - [`mutations`] — flag/state changes + feedback + reactions.
//! - [`search`] — keyword + semantic search.
//!
//! mod.rs keeps types, helpers, and tests. Sub-modules
//! `pub(crate) use *;` so the web router can import handlers by
//! the original `super::conversations::FN` path.

mod mutations;
mod queries;
mod search;

pub(crate) use mutations::*;
pub(crate) use queries::*;
pub(crate) use search::*;

use std::collections::HashMap;

use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use crate::message_util;

/// Wrap a cached JSON body as an axum Response with content-type set.
fn cached_json_response(body: String) -> Response {
    ([(header::CONTENT_TYPE, "application/json")], body).into_response()
}

#[derive(Serialize)]
pub(crate) struct ConversationResponse {
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
    pub last_sender: String,
    pub received_count: u32,
    pub sent_count: u32,
}

#[derive(Serialize)]
pub(crate) struct ThreadMessageResponse {
    pub id: i64,
    pub uid: u32,
    pub sender: String,
    pub recipients: String,
    pub subject: String,
    pub flags: u32,
    pub internal_date: i64,
    pub message_id: String,
    pub text_body: Option<String>,
    pub html_body: Option<String>,
    pub attachments: Vec<crate::message_util::AttachmentInfo>,
    pub category: String,
    pub risk_score: u8,
    pub risk_reason: String,
    pub summary: String,
    pub people: serde_json::Value,
    pub dates: serde_json::Value,
    pub amounts: serde_json::Value,
    pub action_items: serde_json::Value,
    pub ai_analyzed: bool,
    pub clean_text: Option<String>,
    pub new_content: Option<String>,
    pub importance_level: String,
    pub importance_score: f32,
    pub is_bulk_sender: bool,
    pub has_tracking_pixel: bool,
    pub requires_action: bool,
    pub sender_intent: String,
    pub action_deadline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_data: Option<mailrs_intelligence::structured::StructuredData>,
    /// MRS-18: cheap signal so the web client can decide whether to mount
    /// the invite-card without re-parsing attachments client-side. NULL
    /// means "not an invite". Populated either by MRS-4 inbound-pipeline
    /// or by MRS-14 lazy on-read backfill on the message-detail endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invite_method: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct CategoryCount {
    pub category: String,
    pub count: i64,
}

#[derive(Serialize)]
pub(crate) struct SemanticSearchResult {
    pub thread_id: String,
    pub similarity: f64,
}

#[derive(Deserialize)]
pub(crate) struct ConversationsQuery {
    #[serde(default = "super::default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub before: Option<i64>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub domains: Option<String>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(default)]
    pub unread: Option<bool>,
    #[serde(default)]
    pub starred: Option<bool>,
    #[serde(default)]
    pub section: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct SearchQuery {
    pub q: String,
    #[serde(default = "super::default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub domains: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct ContactsQuery {
    #[serde(default)]
    pub q: String,
    #[serde(default = "default_contacts_limit")]
    pub limit: u32,
}

fn default_contacts_limit() -> u32 {
    20
}

pub(crate) fn convos_to_response(
    convos: Vec<mailrs_mailbox::ConversationSummary>,
) -> Vec<ConversationResponse> {
    convos
        .into_iter()
        .map(|c| ConversationResponse {
            thread_id: c.thread_id,
            subject: message_util::decode_header(&c.subject),
            participants: c
                .participants
                .split(',')
                .map(|s| message_util::decode_header(s.trim()))
                .collect(),
            message_count: c.message_count,
            unread_count: c.unread_count,
            last_date: c.last_date,
            category: c.category,
            flagged: c.flagged,
            snippet: c.snippet,
            pinned: c.pinned,
            archived: c.archived,
            importance_level: c.importance_level,
            importance_score: c.importance_score,
            requires_action: c.requires_action,
            last_sender: message_util::decode_header(&c.last_sender),
            // received = total - sent; the SQL only emits sent_count to keep
            // the row tuple under sqlx's 16-column FromRow limit
            received_count: c.message_count.saturating_sub(c.sent_count),
            sent_count: c.sent_count,
        })
        .collect()
}

// ---- snooze API ----

#[derive(Deserialize)]
pub(crate) struct SnoozeRequest {
    pub until: String,
}

// ---- feedback API ----

#[derive(Deserialize)]
pub(crate) struct FeedbackRequest {
    pub sender_email: String,
    pub action: String,
}

const VALID_FEEDBACK_ACTIONS: &[&str] = &[
    "mark_important",
    "mark_vip",
    "mark_spam",
    "block",
    "archive",
    "unblock",
];

// ---- mail stats for dashboard ----

#[derive(Deserialize, Serialize)]
pub(crate) struct MailStats {
    pub total_messages: i64,
    pub unread_messages: i64,
    pub storage_bytes: u64,
    pub categories: Vec<CategoryCount>,
}

/// Valkey TTL for `/api/mail/stats` payload — short enough that user-perceived
/// staleness is bounded, long enough to absorb the dashboard's 60 s refresh
/// loop and any tab-focus refetches. perfs/topics/02.
const MAIL_STATS_TTL_SECS: u64 = 30;

/// batch action type
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BatchAction {
    Read,
    Unread,
    Delete,
    Star,
    Unstar,
    Archive,
    Unarchive,
}

#[derive(Deserialize)]
pub(crate) struct BatchRequest {
    pub thread_ids: Vec<String>,
    pub action: BatchAction,
}

#[derive(Serialize)]
pub(crate) struct BatchResult {
    pub success: bool,
    pub processed: usize,
    pub failed: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

// ---- reactions API ----

#[derive(Deserialize)]
pub(crate) struct ToggleReactionRequest {
    pub emoji: String,
}

#[derive(Serialize)]
pub(crate) struct ReactionSummary {
    pub emoji: String,
    pub count: i64,
    pub me: bool,
}

#[derive(Serialize)]
pub(crate) struct ToggleReactionResponse {
    pub reactions: Vec<ReactionSummary>,
}

#[derive(Serialize)]
pub(crate) struct ThreadReactionsResponse {
    pub reactions: HashMap<i64, Vec<ReactionSummary>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- convos_to_response: response structure for agent consumption ---

    fn make_summary(
        thread_id: &str,
        subject: &str,
        participants: &str,
    ) -> mailrs_mailbox::ConversationSummary {
        mailrs_mailbox::ConversationSummary {
            thread_id: thread_id.to_string(),
            subject: subject.to_string(),
            participants: participants.to_string(),
            message_count: 3,
            unread_count: 1,
            last_date: 1700000000,
            category: "personal".to_string(),
            flagged: false,
            snippet: "hello world".to_string(),
            pinned: false,
            archived: false,
            importance_level: "normal".to_string(),
            importance_score: 0.5,
            requires_action: false,
            last_sender: participants
                .split(',')
                .next()
                .unwrap_or("")
                .trim()
                .to_string(),
            sent_count: 0,
        }
    }

    #[test]
    fn convos_to_response_maps_all_fields() {
        let input = vec![make_summary(
            "thread-1",
            "Test Subject",
            "alice@example.com",
        )];
        let result = convos_to_response(input);

        assert_eq!(result.len(), 1);
        let r = &result[0];
        assert_eq!(r.thread_id, "thread-1");
        assert_eq!(r.subject, "Test Subject");
        assert_eq!(r.participants, vec!["alice@example.com"]);
        assert_eq!(r.message_count, 3);
        assert_eq!(r.unread_count, 1);
        assert_eq!(r.last_date, 1700000000);
        assert_eq!(r.category, "personal");
        assert!(!r.flagged);
        assert_eq!(r.snippet, "hello world");
        assert!(!r.pinned);
        assert!(!r.archived);
        assert_eq!(r.importance_level, "normal");
        assert!((r.importance_score - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn convos_to_response_splits_participants() {
        let input = vec![make_summary(
            "thread-2",
            "Multi",
            "alice@a.com, bob@b.com, carol@c.com",
        )];
        let result = convos_to_response(input);
        assert_eq!(
            result[0].participants,
            vec!["alice@a.com", "bob@b.com", "carol@c.com"]
        );
    }

    #[test]
    fn convos_to_response_empty_input() {
        let result = convos_to_response(vec![]);
        assert!(result.is_empty());
    }

    #[test]
    fn convos_to_response_multiple_conversations() {
        let input = vec![
            make_summary("t1", "First", "a@a.com"),
            make_summary("t2", "Second", "b@b.com"),
            make_summary("t3", "Third", "c@c.com"),
        ];
        let result = convos_to_response(input);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].thread_id, "t1");
        assert_eq!(result[1].thread_id, "t2");
        assert_eq!(result[2].thread_id, "t3");
    }

    // --- conversation response JSON shape for agent consumption ---

    #[test]
    fn conversation_response_serializes_all_agent_fields() {
        let r = ConversationResponse {
            thread_id: "t1".to_string(),
            subject: "Test".to_string(),
            participants: vec!["user@example.com".to_string()],
            message_count: 5,
            unread_count: 2,
            last_date: 1700000000,
            category: "personal".to_string(),
            flagged: true,
            snippet: "preview text".to_string(),
            pinned: false,
            archived: false,
            importance_level: "high".to_string(),
            importance_score: 0.9,
            requires_action: true,
            last_sender: "user@example.com".to_string(),
            received_count: 4,
            sent_count: 1,
        };

        let json = serde_json::to_value(&r).unwrap();

        // verify all fields agents need are present
        assert!(json.get("thread_id").is_some());
        assert!(json.get("subject").is_some());
        assert!(json.get("participants").is_some());
        assert!(json.get("message_count").is_some());
        assert!(json.get("unread_count").is_some());
        assert!(json.get("last_date").is_some());
        assert!(json.get("category").is_some());
        assert!(json.get("flagged").is_some());
        assert!(json.get("snippet").is_some());
        assert!(json.get("pinned").is_some());
        assert!(json.get("archived").is_some());
        assert!(json.get("importance_level").is_some());
        assert!(json.get("importance_score").is_some());

        // verify types
        assert!(json["participants"].is_array());
        assert!(json["message_count"].is_number());
        assert!(json["last_date"].is_number());
        assert!(json["flagged"].is_boolean());
    }

    // --- thread message response JSON shape ---

    #[test]
    fn thread_message_response_serializes_body_fields() {
        let r = ThreadMessageResponse {
            id: 1,
            uid: 100,
            sender: "alice@example.com".to_string(),
            recipients: "bob@example.com".to_string(),
            subject: "Test".to_string(),
            flags: 0,
            internal_date: 1700000000,
            message_id: "<msg1@example.com>".to_string(),
            text_body: Some("plain text content".to_string()),
            html_body: Some("<p>html content</p>".to_string()),
            attachments: vec![],
            category: "personal".to_string(),
            risk_score: 0,
            risk_reason: String::new(),
            summary: String::new(),
            people: serde_json::json!([]),
            dates: serde_json::json!([]),
            amounts: serde_json::json!([]),
            action_items: serde_json::json!([]),
            ai_analyzed: false,
            clean_text: None,
            new_content: None,
            importance_level: "normal".to_string(),
            importance_score: 0.5,
            is_bulk_sender: false,
            has_tracking_pixel: false,
            requires_action: false,
            sender_intent: "inform".to_string(),
            action_deadline: None,
            structured_data: None,
            invite_method: None,
        };

        let json = serde_json::to_value(&r).unwrap();

        // critical fields for agent read operations
        assert!(json.get("text_body").is_some());
        assert!(json.get("html_body").is_some());
        assert!(json.get("attachments").is_some());
        assert!(json.get("sender").is_some());
        assert!(json.get("recipients").is_some());
        assert!(json.get("subject").is_some());
        assert!(json.get("message_id").is_some());

        // verify body content is accessible
        assert_eq!(json["text_body"].as_str().unwrap(), "plain text content");
        assert_eq!(json["html_body"].as_str().unwrap(), "<p>html content</p>");

        // agent intelligence fields
        assert!(json.get("category").is_some());
        assert!(json.get("risk_score").is_some());
        assert!(json.get("summary").is_some());
        assert!(json.get("importance_level").is_some());
        assert!(json.get("requires_action").is_some());
        assert!(json.get("sender_intent").is_some());
    }

    #[test]
    fn thread_message_response_omits_structured_data_when_none() {
        let r = ThreadMessageResponse {
            id: 1,
            uid: 100,
            sender: String::new(),
            recipients: String::new(),
            subject: String::new(),
            flags: 0,
            internal_date: 0,
            message_id: String::new(),
            text_body: None,
            html_body: None,
            attachments: vec![],
            category: "general".to_string(),
            risk_score: 0,
            risk_reason: String::new(),
            summary: String::new(),
            people: serde_json::json!([]),
            dates: serde_json::json!([]),
            amounts: serde_json::json!([]),
            action_items: serde_json::json!([]),
            ai_analyzed: false,
            clean_text: None,
            new_content: None,
            importance_level: "normal".to_string(),
            importance_score: 0.0,
            is_bulk_sender: false,
            has_tracking_pixel: false,
            requires_action: false,
            sender_intent: "inform".to_string(),
            action_deadline: None,
            structured_data: None,
            invite_method: None,
        };

        let json = serde_json::to_value(&r).unwrap();
        // structured_data has skip_serializing_if = "Option::is_none"
        assert!(json.get("structured_data").is_none());
    }

    // --- query parameter deserialization ---

    #[test]
    fn conversations_query_defaults() {
        let q: ConversationsQuery = serde_json::from_str("{}").unwrap();
        assert_eq!(q.limit, 50); // default_limit()
        assert!(q.before.is_none());
        assert!(q.category.is_none());
        assert!(q.domains.is_none());
        assert!(!q.archived);
        assert!(q.folder.is_none());
    }

    #[test]
    fn conversations_query_with_params() {
        let q: ConversationsQuery = serde_json::from_str(
            r#"{"limit":10,"before":1700000000,"category":"personal","folder":"INBOX","archived":true}"#
        ).unwrap();
        assert_eq!(q.limit, 10);
        assert_eq!(q.before, Some(1700000000));
        assert_eq!(q.category.as_deref(), Some("personal"));
        assert_eq!(q.folder.as_deref(), Some("INBOX"));
        assert!(q.archived);
    }

    #[test]
    fn search_query_requires_q() {
        let result: Result<SearchQuery, _> = serde_json::from_str("{}");
        assert!(result.is_err(), "search query should require 'q' field");
    }

    #[test]
    fn search_query_with_defaults() {
        let q: SearchQuery = serde_json::from_str(r#"{"q":"invoice"}"#).unwrap();
        assert_eq!(q.q, "invoice");
        assert_eq!(q.limit, 50);
        assert!(q.category.is_none());
        assert!(q.domains.is_none());
    }

    #[test]
    fn search_query_with_all_params() {
        let q: SearchQuery = serde_json::from_str(
            r#"{"q":"payment","limit":5,"category":"personal","domains":"example.com"}"#,
        )
        .unwrap();
        assert_eq!(q.q, "payment");
        assert_eq!(q.limit, 5);
        assert_eq!(q.category.as_deref(), Some("personal"));
        assert_eq!(q.domains.as_deref(), Some("example.com"));
    }

    // --- superadmin domain access via API key (validates phase 1 integration) ---

    #[test]
    fn superadmin_api_key_grants_domain_access() {
        use crate::api_key_store::{self, CachedApiKey};
        use crate::permission::{AccountGroup, GroupInfo, compute_effective_permissions};

        let (full_key, _prefix, key_hash) = api_key_store::generate_api_key();
        let cached = CachedApiKey {
            key_hash,
            account_address: "admin@golia.jp".to_string(),
            expires_at: None,
            id: 1,
            app_id: None,
        };

        // verify key hash matches
        let token_hash = api_key_store::sha256_hex(full_key.as_bytes());
        assert_eq!(token_hash, cached.key_hash);

        // simulate super user permissions
        let groups = vec![AccountGroup {
            group: GroupInfo {
                id: 1,
                name: "super".into(),
                domain: None,
                description: String::new(),
                is_builtin: true,
                created_at: 0,
            },
            permissions: crate::permission::ALL_PERMISSIONS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }];
        let perms =
            compute_effective_permissions(&groups, &[], &["golia.jp".into(), "example.com".into()]);

        let result = super::super::validate_domains(Some("golia.jp,example.com"), &perms);
        assert_eq!(
            result,
            Some(vec!["golia.jp".to_string(), "example.com".to_string()])
        );
    }

    #[test]
    fn non_superadmin_cannot_access_other_domains() {
        let perms = crate::permission::compute_effective_permissions(&[], &[], &[]);
        let result = super::super::validate_domains(Some("golia.jp"), &perms);
        assert!(result.is_none());
    }

    // --- category count serialization ---

    #[test]
    fn category_count_serializes_correctly() {
        let cc = CategoryCount {
            category: "personal".to_string(),
            count: 42,
        };
        let json = serde_json::to_value(&cc).unwrap();
        assert_eq!(json["category"], "personal");
        assert_eq!(json["count"], 42);
    }
}
