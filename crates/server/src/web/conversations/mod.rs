//! `/api/conversations*` web handlers, split by concern:
//!
//! - [`queries`] — read-only fetches.
//! - [`mutations`] — flag/state changes + feedback + reactions.
//! - [`search`] — keyword + semantic search.
//!
//! mod.rs keeps types, helpers, and tests. Sub-modules
//! `pub(crate) use *;` so the web router can import handlers by
//! the original `super::conversations::FN` path.

mod queries;
mod mutations;
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
#[path = "mod_tests.rs"]
mod tests;
