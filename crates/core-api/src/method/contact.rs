//! Contact + sender_feedback endpoints.
//!
//! Sources:
//! - `crates/mailbox/src/pg/contact_ops.rs` — 6 fn
//! - `crates/mailbox/src/pg/search_ops.rs:230` — `search_contacts`

use serde::{Deserialize, Serialize};

// ── paths ───────────────────────────────────────────────────────────

pub const PATH_SEARCH_CONTACTS: &str = "/v1/users/{user}/contacts:search";
pub const PATH_UPSERT_INBOUND: &str = "/v1/users/{user}/contacts/{email}/inbound";
pub const PATH_UPSERT_OUTBOUND: &str = "/v1/users/{user}/contacts/{email}/outbound";
pub const PATH_MARK_MUTUAL: &str = "/v1/users/{user}/contacts/{email}/mutual";
pub const PATH_CONTACT_SCORING: &str = "/v1/users/{user}/contacts/{email}/scoring";
pub const PATH_HAS_SENT_TO: &str = "/v1/users/{user}/contacts/{email}/has-sent-to";
pub const PATH_SENDER_FEEDBACK: &str = "/v1/users/{user}/contacts/{email}/feedback";

// ── req/resp ────────────────────────────────────────────────────────

/// Query for `GET /v1/users/{user}/contacts:search?q=&limit=`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchContactsQuery {
    /// Substring to ILIKE against `sender` (case-insensitive).
    pub q: String,
    /// Max results.
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_limit() -> u32 {
    5
}

/// Response — list of sender strings most recently used.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchContactsResponse {
    /// Sender strings ordered by `MAX(internal_date)` DESC.
    pub items: Vec<String>,
}

/// Request body for `POST /v1/users/{user}/contacts/{email}/inbound`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertInboundContactRequest {
    /// Display name from From: header.
    pub display_name: String,
    /// True if From: domain matched mailing-list / list-unsubscribe patterns.
    pub is_mailing_list: bool,
    /// True if From: looked automated (no-reply / noreply / etc.).
    pub is_automated: bool,
}

/// Request body for `POST /v1/users/{user}/contacts/{email}/outbound`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertOutboundContactRequest {
    /// Display name observed.
    pub display_name: String,
}

/// Response body for the contact-scoring endpoint.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq)]
pub struct ContactScoring {
    /// True if both sides have sent.
    pub is_mutual: bool,
    /// True if mailing-list-like sender.
    pub is_mailing_list: bool,
    /// True if marked VIP by user.
    pub is_vip: bool,
    /// True if blocked.
    pub is_blocked: bool,
    /// Manual importance bias in [-1.0, 1.0].
    pub importance_bias: f32,
    /// Count of inbound messages.
    pub received_count: u32,
    /// Count of outbound messages.
    pub sent_count: u32,
}

/// Response body for "has user previously sent to this email?"
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct HasSentToResponse {
    /// `true` if the user has any outbound row for this contact.
    pub has_sent: bool,
}

/// Request body for `POST /v1/users/{user}/contacts/{email}/feedback`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SenderFeedbackRequest {
    /// Action label: `block` / `unblock` / `mark_vip` / `unmark_vip` / etc.
    pub action: String,
    /// Optional importance_bias delta to apply (e.g. -0.3 on block).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bias_delta: Option<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_default_limit_is_5() {
        let q: SearchContactsQuery = serde_json::from_str(r#"{"q":"chime"}"#).unwrap();
        assert_eq!(q.limit, 5);
    }

    #[test]
    fn scoring_defaults() {
        let s = ContactScoring::default();
        assert!(!s.is_mutual);
        assert_eq!(s.received_count, 0);
    }

    #[test]
    fn feedback_omits_none_delta() {
        let f = SenderFeedbackRequest {
            action: "block".into(),
            bias_delta: None,
        };
        let s = serde_json::to_string(&f).unwrap();
        assert!(!s.contains("bias_delta"));
    }
}
