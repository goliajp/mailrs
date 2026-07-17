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

/// `GET /v1/users/{user}/sent-messages` — one row per message the user
/// actually sent (not per thread), newest first, so the Sent view can
/// show the recipient and every outbound message in a multi-reply thread
/// separately.
pub const PATH_LIST_SENT_MESSAGES: &str = "/v1/users/{user}/sent-messages";

// ── mutate paths (every action becomes its own POST) ───────────────

pub const PATH_MARK_READ: &str = "/v1/users/{user}/threads/{thread_id}/read";
pub const PATH_MARK_UNREAD: &str = "/v1/users/{user}/threads/{thread_id}/unread";
pub const PATH_MARK_ALL_READ: &str = "/v1/users/{user}/conversations:mark-all-read";
pub const PATH_STAR: &str = "/v1/users/{user}/threads/{thread_id}/star";
pub const PATH_UNSTAR: &str = "/v1/users/{user}/threads/{thread_id}/unstar";
pub const PATH_PIN: &str = "/v1/users/{user}/threads/{thread_id}/pin";
pub const PATH_UNPIN: &str = "/v1/users/{user}/threads/{thread_id}/unpin";
pub const PATH_ARCHIVE: &str = "/v1/users/{user}/threads/{thread_id}/archive";
pub const PATH_UNARCHIVE: &str = "/v1/users/{user}/threads/{thread_id}/unarchive";
pub const PATH_SNOOZE: &str = "/v1/users/{user}/threads/{thread_id}/snooze";
pub const PATH_UNSNOOZE: &str = "/v1/users/{user}/threads/{thread_id}/unsnooze";
pub const PATH_DELETE_THREAD: &str = "/v1/users/{user}/threads/{thread_id}";

/// v2.4.1 Phase 3 (RFC-B §3.4) — mark a thread as junk.
/// Route + fastcore handler: move to `user_threads_junk` zset,
/// stamp `category = "spam"` on the row. Does NOT modify the
/// recipient's whitelist / blacklist (per §D4 — "只移这封,不动名单").
pub const PATH_MARK_JUNK: &str = "/v1/users/{user}/threads/{thread_id}/mark-junk";

/// v2.4.1 Phase 3 (RFC-B §3.4) — mark a thread as not junk.
/// Route + fastcore handler: move to `user_threads_inbox` zset,
/// stamp `category = "inbox"`. The webapi handler additionally
/// SADDs the thread's sender into `spam:{user}:whitelist` so
/// future arrivals from the same sender bypass the score
/// threshold when authed.
pub const PATH_MARK_NOT_JUNK: &str = "/v1/users/{user}/threads/{thread_id}/mark-not-junk";

/// v2.9 triage — move a thread into the Notifications bucket
/// (`set_bucket(Notifications)`, stamp `category = "notification"`),
/// and train the triage classifier on this correction.
pub const PATH_MARK_NOTIFICATION: &str = "/v1/users/{user}/threads/{thread_id}/mark-notification";

/// v2.9 triage — move a thread into the Promotions bucket
/// (`set_bucket(Promotions)`, stamp `category = "promotion"`), and
/// train the triage classifier on this correction.
pub const PATH_MARK_PROMOTION: &str = "/v1/users/{user}/threads/{thread_id}/mark-promotion";

/// v2.9 triage — move a thread into the Inbox bucket
/// (`set_bucket(Inbox)`, stamp `category = "inbox"`), and train the
/// triage classifier on this correction. Also clears junk if the
/// thread was previously in Junk.
pub const PATH_MOVE_TO_INBOX: &str = "/v1/users/{user}/threads/{thread_id}/move-to-inbox";

/// `POST /v1/users/{user}/threads/{thread_id}/messages` — deliver a
/// synthesized message (sent copy, saved draft, imported item) into the
/// user's kevy view. Fires the same `record_message_arrival` +
/// `upsert_thread` sequence used by inbound delivery so the sent index,
/// activity zset, and message blob all populate together. This is the
/// write endpoint the webapi send/save-draft handlers call after
/// enqueueing outbound / writing maildir.
pub const PATH_DELIVER_MESSAGE: &str = "/v1/users/{user}/threads/{thread_id}/messages";

// ── req/resp ────────────────────────────────────────────────────────

/// Response body for `GET /v1/users/{user}/threads/{thread_id}/messages`.
///
/// `MessageWire` defined in `method::message`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListThreadMessagesResponse {
    /// Message rows in this thread, ordered by `internal_date` ASC.
    pub items: Vec<crate::method::message::MessageWire>,
}

/// One outbound message in the Sent view — message granularity, not
/// thread. Carries the recipient (To) so the list shows who it went to,
/// and `thread_id` + `uid` so a click opens the thread and focuses this
/// exact message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentMessageSummary {
    pub uid: u32,
    pub message_id: String,
    pub thread_id: String,
    /// Raw `To:` header — who this message was sent to.
    pub to: String,
    pub subject: String,
    /// Epoch seconds.
    pub internal_date: i64,
}

/// Response body for `GET /v1/users/{user}/sent-messages`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentMessagesResponse {
    pub items: Vec<SentMessageSummary>,
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

/// Request body for `POST /v1/users/{user}/threads/{thread_id}/messages`.
///
/// The webapi caller has already produced the RFC 5322 envelope (for
/// send/reply) or the draft body (for save-draft), delivered it to the
/// maildir (`.Sent` / `.Drafts`), and holds the resulting blob_ref
/// (the maildir filename). Here it hands the fastcore side the
/// metadata needed to:
///
///   - populate `mailrs:thread:<tid>` aggregate + `sent_count`
///   - add the tid to `mailrs:user:<u>:threads:sent` (via `upsert_thread`
///     with `senders_csv` containing `user`)
///   - write the per-message `mailrs:msg:<mid>` JSON blob so
///     `list_thread_messages` returns it
///   - index the UID in `mailrs:user:<u>:msg_by_uid`
///
/// `payload_wire_json` is the pre-serialized `MessageWire` — fastcore
/// stores it verbatim; the enrichment path in the webapi
/// (`enrich_with_body`) then reads the maildir file at `blob_ref`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliverMessageRequest {
    /// RFC 5322 `Message-ID:` header value (no angle brackets).
    pub message_id: String,
    /// Decoded subject; overwritten into the thread aggregate.
    pub subject: String,
    /// Comma-joined sender addresses; drives the sent-index membership
    /// check (`senders_csv_contains_user`).
    pub senders_csv: String,
    /// Epoch seconds for the thread's `latest_date` + activity zset score.
    pub latest_date: i64,
    /// Short preview (first ~120 chars of body); overwritten into the
    /// thread aggregate.
    pub latest_preview: String,
    /// Category label used by `user_threads_by_category`. For sent /
    /// draft the caller should pass `"inbox"` so it doesn't drop into a
    /// spam/scam bucket by accident.
    pub category: String,
    /// `true` = inbound-arrival (bumps `unread_count`); `false` = sent
    /// or draft (bumps `sent_count`).
    pub unread: bool,
    /// UID within the user's mailbox — must match
    /// `payload_wire_json.uid`. Fastcore mirrors it into the
    /// user-scoped uid index so `get_message_by_uid_for_user` finds it.
    pub uid: u32,
    /// Pre-serialized `MessageWire` (JSON). Stored verbatim as the
    /// message blob.
    pub payload_wire_json: String,
}

/// Response body for `DeliverMessageRequest`. Currently just echoes
/// the resolved `thread_id` for symmetry with other wire types; a
/// future field could carry the new modseq once fastcore honours
/// CONDSTORE.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DeliverMessageResponse {
    pub thread_id: String,
    pub message_id: String,
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
