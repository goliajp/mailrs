//! email_analysis + semantic search endpoints.
//!
//! Sources:
//! - `crates/mailbox/src/pg/analysis_ops.rs`         — 5 fn
//! - `crates/mailbox/src/pg/search_ops.rs:258`       — `semantic_search` (Rock 3, pgvector)
//! - `crates/mailbox/src/pg/attachment_ops.rs:5`     — `get_attachment_texts`
//!
//! `semantic_search`: fastcore v1 returns `BackendUnsupported` per checklist 7.7.
//! Subsequent version connects meili arroy field.

use serde::{Deserialize, Serialize};

use crate::types::{MessageId, ThreadId, UserAddress};

// ── paths ───────────────────────────────────────────────────────────

pub const PATH_GET_ANALYSIS: &str = "/v1/analysis/{message_id}";
pub const PATH_UPSERT_ANALYSIS: &str = "/v1/analysis/{message_id}";
pub const PATH_LIST_UNANALYZED: &str = "/v1/analysis:unanalyzed";
pub const PATH_COUNT_UNANALYZED: &str = "/v1/analysis:unanalyzed-count";
pub const PATH_BOOST_IMPORTANCE: &str = "/v1/analysis/{message_id}/boost-importance";
pub const PATH_SEMANTIC_SEARCH: &str = "/v1/search/semantic";
pub const PATH_ATTACHMENT_TEXTS: &str = "/v1/messages/{id}/attachments/texts";

// ── wire types ──────────────────────────────────────────────────────

/// Wire mirror of `mailrs_mailbox::types::EmailAnalysisRow`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailAnalysisWire {
    pub message_id: MessageId,
    pub category: String,
    pub risk_score: i16,
    pub risk_reason: String,
    pub summary: String,
    pub people: serde_json::Value,
    pub dates: serde_json::Value,
    pub amounts: serde_json::Value,
    pub action_items: serde_json::Value,
    pub model_version: String,
    pub clean_text: String,
    pub requires_action: bool,
    pub sender_intent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_deadline: Option<String>,
}

// ── req/resp ────────────────────────────────────────────────────────

/// Response body for `GET /v1/analysis/{message_id}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetAnalysisResponse {
    /// Analysis row, or `null` if message has not been analyzed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analysis: Option<EmailAnalysisWire>,
}

/// Request body for `PUT /v1/analysis/{message_id}` (upsert).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertAnalysisRequest {
    /// All analysis fields. Embedding bytes are base64-encoded raw f32 vector.
    pub row: EmailAnalysisWire,
    /// 768-dim or similar embedding, raw little-endian f32, base64.
    /// `None` skips writing embedding (fastcore may use meili instead).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding_base64: Option<String>,
}

/// Query for `GET /v1/analysis:unanalyzed?limit=&model_version=`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListUnanalyzedQuery {
    /// Max items.
    #[serde(default = "default_unanalyzed_limit")]
    pub limit: u32,
    /// Current model version — rows with `model_version != X` are also
    /// considered "unanalyzed" (need re-analysis).
    pub model_version: String,
}

fn default_unanalyzed_limit() -> u32 {
    50
}

/// One row in the unanalyzed list — minimal fields the analyzer worker
/// needs to build its prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnanalyzedMessageRow {
    pub message_id: MessageId,
    pub user_address: UserAddress,
    pub maildir_id: String,
    pub sender: String,
    pub subject: String,
}

/// Response body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListUnanalyzedResponse {
    pub items: Vec<UnanalyzedMessageRow>,
}

/// Response body for the count endpoint.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct CountUnanalyzedResponse {
    pub count: i64,
}

/// Request body for `POST /v1/search/semantic`.
///
/// fastcore v1 returns `BackendUnsupported`; webapi falls back to meili.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticSearchRequest {
    /// User scope.
    pub user: UserAddress,
    /// Query embedding (base64-encoded little-endian f32 vector).
    pub query_embedding_base64: String,
    /// Max items.
    #[serde(default = "default_semantic_limit")]
    pub limit: u32,
}

fn default_semantic_limit() -> u32 {
    10
}

/// One semantic-search hit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticSearchHit {
    pub message_id: MessageId,
    pub thread_id: ThreadId,
    /// Cosine similarity in [0.0, 1.0]; higher is closer.
    pub similarity: f32,
}

/// Response body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticSearchResponse {
    pub items: Vec<SemanticSearchHit>,
}

/// Response body for `GET /v1/messages/{id}/attachments/texts`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentTextsResponse {
    /// One string per attachment with non-empty extracted text, ordered by
    /// attachment_index ASC. May be empty.
    pub texts: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_search_default_limit() {
        let s = r#"{"user":"u@x.com","query_embedding_base64":"aGVsbG8="}"#;
        let r: SemanticSearchRequest = serde_json::from_str(s).unwrap();
        assert_eq!(r.limit, 10);
    }

    #[test]
    fn count_unanalyzed_roundtrip() {
        let r = CountUnanalyzedResponse { count: 99 };
        let s = serde_json::to_string(&r).unwrap();
        let back: CountUnanalyzedResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn analysis_row_jsonb_passthrough() {
        let w = EmailAnalysisWire {
            message_id: 1,
            category: "personal".into(),
            risk_score: 10,
            risk_reason: "".into(),
            summary: "hi".into(),
            people: serde_json::json!(["alice"]),
            dates: serde_json::json!([]),
            amounts: serde_json::json!([]),
            action_items: serde_json::json!(["reply"]),
            model_version: "qwen3.5-9b/v1".into(),
            clean_text: "".into(),
            requires_action: true,
            sender_intent: "inform".into(),
            action_deadline: None,
        };
        let s = serde_json::to_string(&w).unwrap();
        assert!(s.contains("\"people\":[\"alice\"]"));
        assert!(!s.contains("action_deadline"));
    }
}
