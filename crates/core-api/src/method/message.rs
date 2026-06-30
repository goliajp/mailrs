//! Message read / mutate / index endpoints.
//!
//! Sources:
//! - `crates/mailbox/src/pg/message_ops/read.rs`   — 10 fn
//! - `crates/mailbox/src/pg/message_ops/mutate.rs` — 6 fn
//! - `crates/mailbox/src/pg/message_ops/index.rs`  — 4 fn
//! - `crates/mailbox/src/pg/flag_ops.rs`           — 5 fn (incl. CONDSTORE)

use serde::{Deserialize, Serialize};

use crate::types::{MailboxId, MaildirId, MessageId, ThreadId, UserAddress};

// ── paths ───────────────────────────────────────────────────────────

pub const PATH_GET_MESSAGE: &str = "/v1/messages/{id}";
pub const PATH_GET_MESSAGE_RAW: &str = "/v1/messages/{id}/raw";
pub const PATH_GET_MESSAGE_BY_UID: &str = "/v1/mailboxes/{id}/messages/uid/{uid}";
pub const PATH_LIST_MESSAGES: &str = "/v1/mailboxes/{id}/messages";
pub const PATH_FIND_BY_MESSAGE_ID: &str = "/v1/users/{user}/messages/by-message-id/{message_id}";
pub const PATH_QUERY_MESSAGES: &str = "/v1/users/{user}/messages:query";
pub const PATH_INSERT_MESSAGE: &str = "/v1/mailboxes/{id}/messages";
pub const PATH_EXPUNGE: &str = "/v1/mailboxes/{id}/expunge";
pub const PATH_COPY_MESSAGE: &str = "/v1/users/{user}/mailboxes/{src_id}/messages/{uid}/copy";
pub const PATH_MOVE_MESSAGE: &str = "/v1/users/{user}/mailboxes/{src_id}/messages/{uid}/move";
pub const PATH_SET_FLAGS: &str = "/v1/mailboxes/{id}/messages/{uid}/flags";
pub const PATH_FLAGS_IF_UNCHANGED: &str = "/v1/mailboxes/{id}/messages/{uid}/condstore";
pub const PATH_CHANGED_SINCE: &str = "/v1/mailboxes/{id}/changed-since/{modseq}";
pub const PATH_INVITE_PAYLOAD: &str = "/v1/messages/{id}/invite-payload";
pub const PATH_GET_INVITE_METHODS: &str = "/v1/invites/by-message-ids";

// ── wire types ──────────────────────────────────────────────────────

/// Wire mirror of `mailrs_mailbox::types::Message`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageWire {
    /// Store-native primary key.
    pub id: MessageId,
    /// FK into mailboxes.
    pub mailbox_id: MailboxId,
    /// IMAP UID within mailbox_id.
    pub uid: u32,
    /// Opaque body reference (maildir filename for the PG impl).
    pub blob_ref: MaildirId,
    /// Raw `From:` header.
    pub sender: String,
    /// Raw `To:` header (comma-separated).
    pub recipients: String,
    /// Decoded `Subject:`.
    pub subject: String,
    /// `Date:` epoch seconds.
    pub date: i64,
    /// Server-side delivery time, epoch seconds.
    pub internal_date: i64,
    /// Size in bytes.
    pub size: u32,
    /// Flag bitmask. See `mailrs_mailbox::types::FLAG_*` constants.
    pub flags: u32,
    /// RFC 5322 `Message-ID:` (no angle brackets).
    pub message_id: String,
    /// RFC 5322 `In-Reply-To:` (no angle brackets).
    pub in_reply_to: String,
    /// Resolved thread identifier.
    pub thread_id: ThreadId,
    /// CONDSTORE per-message MODSEQ.
    pub modseq: u64,
    /// Owner email address. Optional because some inherent methods return
    /// `MessageMeta` which doesn't carry it (cement handlers fill it from
    /// the request path when known, else leave blank).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub user_address: UserAddress,
}

impl From<&mailrs_mailbox::types::MessageMeta> for MessageWire {
    fn from(m: &mailrs_mailbox::types::MessageMeta) -> Self {
        Self {
            id: m.id,
            mailbox_id: m.mailbox_id,
            uid: m.uid,
            blob_ref: m.maildir_id.clone(),
            sender: m.sender.clone(),
            recipients: m.recipients.clone(),
            subject: m.subject.clone(),
            date: m.date,
            internal_date: m.internal_date,
            size: m.size,
            flags: m.flags,
            message_id: m.message_id.clone(),
            in_reply_to: m.in_reply_to.clone(),
            thread_id: m.thread_id.clone(),
            modseq: m.modseq,
            // MessageMeta has no user_address — caller fills if needed.
            user_address: String::new(),
        }
    }
}

impl From<&mailrs_mailbox::types::Message> for MessageWire {
    fn from(m: &mailrs_mailbox::types::Message) -> Self {
        Self {
            id: m.id,
            mailbox_id: m.mailbox_id,
            uid: m.uid,
            blob_ref: m.blob_ref.clone(),
            sender: m.sender.clone(),
            recipients: m.recipients.clone(),
            subject: m.subject.clone(),
            date: m.date,
            internal_date: m.internal_date,
            size: m.size,
            flags: m.flags,
            message_id: m.message_id.clone(),
            in_reply_to: m.in_reply_to.clone(),
            thread_id: m.thread_id.clone(),
            modseq: m.modseq,
            user_address: m.user_address.clone(),
        }
    }
}

impl From<mailrs_mailbox::types::Message> for MessageWire {
    fn from(m: mailrs_mailbox::types::Message) -> Self {
        (&m).into()
    }
}

/// Wire mirror of `mailrs_mailbox::types::Inserted`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct InsertedWire {
    /// Newly assigned message id.
    pub id: MessageId,
    /// Allocated UID.
    pub uid: u32,
    /// New MODSEQ.
    pub modseq: u64,
}

impl From<mailrs_mailbox::types::Inserted> for InsertedWire {
    fn from(i: mailrs_mailbox::types::Inserted) -> Self {
        Self {
            id: i.id,
            uid: i.uid,
            modseq: i.modseq,
        }
    }
}

/// Flag mutation op (mirror of `mailrs_mailbox::types::FlagOp`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FlagOpWire {
    /// Replace mask entirely.
    Set,
    /// OR into mask.
    Add,
    /// AND-NOT out of mask.
    Remove,
}

impl From<mailrs_mailbox::types::FlagOp> for FlagOpWire {
    fn from(op: mailrs_mailbox::types::FlagOp) -> Self {
        match op {
            mailrs_mailbox::types::FlagOp::Set => Self::Set,
            mailrs_mailbox::types::FlagOp::Add => Self::Add,
            mailrs_mailbox::types::FlagOp::Remove => Self::Remove,
        }
    }
}

impl From<FlagOpWire> for mailrs_mailbox::types::FlagOp {
    fn from(op: FlagOpWire) -> Self {
        match op {
            FlagOpWire::Set => Self::Set,
            FlagOpWire::Add => Self::Add,
            FlagOpWire::Remove => Self::Remove,
        }
    }
}

// ── req/resp ────────────────────────────────────────────────────────

/// Request body for `POST /v1/mailboxes/{id}/messages`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsertMessageRequest {
    /// Target mailbox name (e.g. `INBOX`).
    pub mailbox_name: String,
    /// Opaque body ref.
    pub blob_ref: MaildirId,
    /// Raw `From:`.
    pub sender: String,
    /// Raw `To:`.
    pub recipients: String,
    /// Decoded `Subject:`.
    pub subject: String,
    /// Size bytes.
    pub size: u32,
    /// `Date:` epoch seconds.
    pub date: i64,
    /// Delivery time epoch seconds.
    pub internal_date: i64,
    /// RFC 5322 `Message-ID:`.
    pub message_id: String,
    /// RFC 5322 `In-Reply-To:`.
    pub in_reply_to: String,
    /// Resolved thread id.
    pub thread_id: ThreadId,
    /// Initial flag bitmask.
    pub flags: u32,
}

/// Response: the new row identity.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct InsertMessageResponse {
    /// What got inserted.
    pub inserted: InsertedWire,
}

/// Request body for list-messages pagination.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct ListMessagesQuery {
    /// Page offset.
    #[serde(default)]
    pub offset: u32,
    /// Max items.
    #[serde(default = "default_list_limit")]
    pub limit: u32,
}

fn default_list_limit() -> u32 {
    50
}

/// Generic items envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListMessagesResponse {
    pub items: Vec<MessageWire>,
}

/// Request body for `POST /v1/users/{user}/messages:query`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueryMessagesRequest {
    /// Optional single-mailbox restriction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mailbox_id: Option<MailboxId>,
    /// Free-text substring search (sender + recipients + subject).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Require this flag bit set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_keyword: Option<u32>,
    /// Require this flag bit unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_keyword: Option<u32>,
    /// Pagination offset.
    #[serde(default)]
    pub position: u32,
    /// Page size.
    #[serde(default = "default_list_limit")]
    pub limit: u32,
}

/// Request body for set/add/remove flags.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FlagMutationRequest {
    /// Operation kind.
    pub op: FlagOpWire,
    /// Mask to apply.
    pub flags: u32,
}

/// Response: new modseq after flag mutation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlagMutationResponse {
    /// New `highest_modseq` of the affected mailbox.
    pub new_modseq: u64,
}

/// Request body for copy/move — destination mailbox name in the same user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopyMoveRequest {
    /// Destination mailbox name (e.g. `Archive`).
    pub dst_mailbox_name: String,
}

/// Response body for copy/move — new UID in the destination mailbox.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct CopyMoveResponse {
    /// Allocated UID in the destination mailbox.
    pub new_uid: u32,
}

/// Response body for expunge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpungeResponse {
    /// UIDs that were expunged (i.e. flagged `\Deleted` and removed).
    pub expunged_uids: Vec<u32>,
}

/// CONDSTORE compare-and-set request.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CondstoreRequest {
    /// Operation kind.
    pub op: FlagOpWire,
    /// Mask to apply.
    pub flags: u32,
    /// Client's last-known modseq for this message.
    pub unchanged_since: u64,
}

/// CONDSTORE response — `Ok(new_modseq)` on success, `Err(actual_modseq)` on
/// conflict.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum CondstoreResponse {
    /// Applied; here's the new modseq.
    Applied {
        /// Post-update modseq.
        new_modseq: u64,
    },
    /// Conflict — the message's modseq is newer than `unchanged_since`.
    Conflict {
        /// Current modseq on the server.
        current_modseq: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_op_wire_serde() {
        let s = serde_json::to_string(&FlagOpWire::Add).unwrap();
        assert_eq!(s, "\"add\"");
        let back: FlagOpWire = serde_json::from_str("\"remove\"").unwrap();
        assert_eq!(back, FlagOpWire::Remove);
    }

    #[test]
    fn inserted_wire_roundtrip() {
        let i = InsertedWire {
            id: 7,
            uid: 100,
            modseq: 42,
        };
        let s = serde_json::to_string(&i).unwrap();
        let back: InsertedWire = serde_json::from_str(&s).unwrap();
        assert_eq!(i, back);
    }

    #[test]
    fn list_messages_query_defaults() {
        let q: ListMessagesQuery = serde_json::from_str("{}").unwrap();
        assert_eq!(q.offset, 0);
        assert_eq!(q.limit, 50);
    }

    #[test]
    fn condstore_response_tagged() {
        let applied = CondstoreResponse::Applied { new_modseq: 99 };
        let s = serde_json::to_string(&applied).unwrap();
        assert!(s.contains("\"outcome\":\"applied\""));
        assert!(s.contains("\"new_modseq\":99"));

        let conflict = CondstoreResponse::Conflict { current_modseq: 50 };
        let s2 = serde_json::to_string(&conflict).unwrap();
        assert!(s2.contains("\"outcome\":\"conflict\""));
        assert!(s2.contains("\"current_modseq\":50"));
    }
}
