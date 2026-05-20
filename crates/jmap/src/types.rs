//! Minimal JMAP-facing data types.
//!
//! These are intentionally decoupled from any backing store (Maildir, IMAP, S3,
//! whatever). A `MailStore` implementation is responsible for mapping its own
//! representation into these shapes.

use serde::{Deserialize, Serialize};

/// Mailbox metadata as needed by JMAP's `Mailbox/get` and `Mailbox/query`.
///
/// `id` is the store's native primary key. The dispatcher renders it to the
/// JMAP-visible id `mb-{id}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mailbox {
    pub id: i64,
    pub name: String,
}

/// Per-mailbox counts returned by [`MailStore::mailbox_status`].
#[derive(Debug, Clone, Copy, Default)]
pub struct MailboxCounts {
    pub total: u32,
    pub unread: u32,
}

/// A single message as JMAP needs to see it for `Email/get`, `Email/query`,
/// `Email/set`, `Thread/get`, and `EmailSubmission/set`.
///
/// JMAP-visible email id is `msg-{id}`.
#[derive(Debug, Clone)]
pub struct Message {
    pub id: i64,
    pub mailbox_id: i64,
    pub uid: u32,
    pub sender: String,
    pub recipients: String,
    pub subject: String,
    /// `Date:` header epoch seconds.
    pub date: i64,
    pub size: u32,
    pub flags: u32,
    /// Internal delivery time epoch seconds.
    pub internal_date: i64,
    pub message_id: String,
    pub in_reply_to: String,
    pub thread_id: String,
    /// Owner's full address (used to read the raw bytes from a store).
    pub user_address: String,
    /// Optional pre-extracted plain-text snippet, used for `preview`.
    pub new_content: Option<String>,
    /// Implementation-defined opaque id used by [`MailStore::read_message_raw`]
    /// to locate the on-disk / blob copy of the message.
    pub blob_id: String,
}

/// Parsed message body parts as returned by [`MailStore::parse_message`].
#[derive(Debug, Clone, Default)]
pub struct ParsedBody {
    pub text: Option<String>,
    pub html: Option<String>,
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone)]
pub struct Attachment {
    pub filename: String,
    pub content_type: String,
    pub size: u32,
}

/// Result of submitting a previously-stored email through the outbound queue.
#[derive(Debug, Clone)]
pub struct SubmissionResult {
    pub success: bool,
    pub message: Option<String>,
}

/// Flag bitmask constants. These mirror the wire-level numbering already used
/// by mailrs-mailbox so a server that wraps mailrs-mailbox can pass flags
/// through without translation.
pub const FLAG_SEEN: u32 = 0b0000_0001;
pub const FLAG_ANSWERED: u32 = 0b0000_0010;
pub const FLAG_FLAGGED: u32 = 0b0000_0100;
pub const FLAG_DELETED: u32 = 0b0000_1000;
pub const FLAG_DRAFT: u32 = 0b0001_0000;
