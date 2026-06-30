//! Thread read / mutate endpoints.
//!
//! Sources:
//! - `crates/mailbox/src/pg/thread_ops/mod.rs`    — 6 fn (read)
//! - `crates/mailbox/src/pg/thread_ops/mutate.rs` — 12 fn (mutate)

use serde::{Deserialize, Serialize};

use crate::types::{MessageId, ThreadId, UserAddress};

// ── read paths ──────────────────────────────────────────────────────

pub const PATH_LIST_THREAD_MESSAGES: &str = "/v1/users/{user}/threads/{thread_id}/messages";
pub const PATH_THREAD_REFS: &str = "/v1/users/{user}/threads/{thread_id}/refs";
pub const PATH_THREAD_LAST_MESSAGE_ID: &str =
    "/v1/users/{user}/threads/{thread_id}/last-message-id";
pub const PATH_FIND_THREAD_BY_MESSAGE_ID: &str =
    "/v1/users/{user}/threads/by-message-id/{message_id}";
pub const PATH_THREAD_MESSAGE_IDS: &str = "/v1/users/{user}/threads/{thread_id}/message-ids";
pub const PATH_BACKFILL_THREADING: &str = "/v1/admin/backfill-threading";

// ── mutate paths (every action becomes its own POST) ───────────────

pub const PATH_MARK_READ: &str = "/v1/users/{user}/threads/{thread_id}/read";
pub const PATH_MARK_UNREAD: &str = "/v1/users/{user}/threads/{thread_id}/unread";
pub const PATH_STAR: &str = "/v1/users/{user}/threads/{thread_id}/star";
pub const PATH_UNSTAR: &str = "/v1/users/{user}/threads/{thread_id}/unstar";
pub const PATH_PIN: &str = "/v1/users/{user}/threads/{thread_id}/pin";
pub const PATH_UNPIN: &str = "/v1/users/{user}/threads/{thread_id}/unpin";
pub const PATH_ARCHIVE: &str = "/v1/users/{user}/threads/{thread_id}/archive";
pub const PATH_UNARCHIVE: &str = "/v1/users/{user}/threads/{thread_id}/unarchive";
pub const PATH_SNOOZE: &str = "/v1/users/{user}/threads/{thread_id}/snooze";
pub const PATH_UNSNOOZE: &str = "/v1/users/{user}/threads/{thread_id}/snooze";
pub const PATH_DELETE_THREAD: &str = "/v1/users/{user}/threads/{thread_id}";
pub const PATH_DISMISS_ACTION: &str = "/v1/users/{user}/threads/{thread_id}/dismiss-action";

// ── req/resp ────────────────────────────────────────────────────────

/// Response body for `GET /v1/users/{user}/threads/{thread_id}/messages`.
///
/// `MessageWire` defined in `method::message`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListThreadMessagesResponse {
    /// Message rows in this thread, ordered by `internal_date` ASC.
    pub items: Vec<crate::method::message::MessageWire>,
}

/// Request body for `PUT /v1/users/{user}/threads/{thread_id}/snooze`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnoozeRequest {
    /// Epoch-seconds when to wake the thread up. `0` = "indefinitely until cleared".
    pub snoozed_until: i64,
}

/// Generic "thread mutate" affected-row count response.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ThreadActionResponse {
    /// Number of messages modified by this action.
    pub affected: u32,
    /// New `highest_modseq` of the affected mailbox (for CONDSTORE clients).
    /// 0 when the action didn't bump modseq (rare).
    pub new_modseq: u64,
}

/// Response body for `GET .../{thread_id}/refs` — RFC 5322 References chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadRefsResponse {
    /// RFC 5322 Message-ID list (without angle brackets) for every message
    /// in the thread, ordered by `internal_date` ASC. Used to build the
    /// `References:` header on reply.
    pub message_ids: Vec<String>,
}

/// Response body for `GET .../{thread_id}/last-message-id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastMessageIdResponse {
    /// RFC 5322 `Message-ID:` of the most recent message in the thread.
    /// May be empty string if the thread is empty / has no header.
    pub message_id: String,
}

/// Response body for `GET .../by-message-id/{message_id}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindThreadByMessageIdResponse {
    /// Resolved thread_id, or `None` if no message with this RFC 5322
    /// `Message-ID:` exists in the user's mailboxes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<ThreadId>,
}

/// Response body for `GET .../{thread_id}/message-ids`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadMessageIdsResponse {
    /// Store-native message IDs in the thread, ordered by `internal_date` ASC.
    pub ids: Vec<MessageId>,
}

/// Request body for the admin `POST /v1/admin/backfill-threading`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BackfillThreadingRequest {
    /// Override the maildir root. `None` = use the value from server config.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maildir_root: Option<String>,
    /// Optional user scope. `None` = backfill across all users.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<UserAddress>,
}

/// Response body for backfill_threading admin op.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct BackfillThreadingResponse {
    /// Number of messages whose thread_id was filled.
    pub repaired: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snooze_request_roundtrip() {
        let req = SnoozeRequest {
            snoozed_until: 1_700_000_000,
        };
        let s = serde_json::to_string(&req).unwrap();
        let back: SnoozeRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn thread_action_default() {
        let r = ThreadActionResponse::default();
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"affected\":0"));
        assert!(s.contains("\"new_modseq\":0"));
    }

    #[test]
    fn refs_response_roundtrip() {
        let r = ThreadRefsResponse {
            message_ids: vec!["a@x.com".into(), "b@y.com".into()],
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: ThreadRefsResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(back.message_ids.len(), 2);
    }

    #[test]
    fn find_thread_omits_none() {
        let r = FindThreadByMessageIdResponse { thread_id: None };
        let s = serde_json::to_string(&r).unwrap();
        assert!(!s.contains("thread_id"));
    }
}
