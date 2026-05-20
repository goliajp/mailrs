//! Minimal JMAP-facing data types.
//!
//! These are intentionally decoupled from any backing store (Maildir, IMAP, S3,
//! whatever). A `MailStore` implementation is responsible for mapping its own
//! representation into these shapes.
//!
//! ## Flag bitmask
//!
//! The `FLAG_*` constants mirror the wire-level numbering already used by
//! mailrs-mailbox, so a server that wraps mailrs-mailbox can pass flags
//! through without translation.

use serde::{Deserialize, Serialize};

/// Mailbox metadata as needed by JMAP's `Mailbox/get` and `Mailbox/query`.
///
/// `id` is the store's native primary key. The dispatcher renders it to the
/// JMAP-visible id `mb-{id}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mailbox {
    /// Store-native primary key. Rendered on the wire as `mb-{id}`.
    pub id: i64,
    /// Human-visible mailbox name (e.g. `INBOX`, `Sent`, `Drafts`).
    pub name: String,
}

/// Per-mailbox counts returned by [`crate::store::MailStore::mailbox_status`].
#[derive(Debug, Clone, Copy, Default)]
pub struct MailboxCounts {
    /// Total number of messages in the mailbox.
    pub total: u32,
    /// Number of messages without the `\Seen` flag.
    pub unread: u32,
}

/// A single message as JMAP needs to see it for `Email/get`, `Email/query`,
/// `Email/set`, `Thread/get`, and `EmailSubmission/set`.
///
/// JMAP-visible email id is `msg-{id}`.
#[derive(Debug, Clone)]
pub struct Message {
    /// Store-native primary key. Rendered on the wire as `msg-{id}`.
    pub id: i64,
    /// FK into the message's containing mailbox.
    pub mailbox_id: i64,
    /// IMAP-style UID within `mailbox_id`. Combined with `mailbox_id` it
    /// uniquely identifies the row for flag updates.
    pub uid: u32,
    /// Raw `From:` header value, may include a display name (e.g.
    /// `"Alice" <alice@example.com>`).
    pub sender: String,
    /// Raw `To:` header value. Comma-separated address list.
    pub recipients: String,
    /// Decoded `Subject:` header.
    pub subject: String,
    /// `Date:` header epoch seconds.
    pub date: i64,
    /// Message size in bytes.
    pub size: u32,
    /// Flag bitmask. See the `FLAG_*` constants in this module.
    pub flags: u32,
    /// Internal delivery time epoch seconds.
    pub internal_date: i64,
    /// `Message-ID:` header value, without the angle brackets.
    pub message_id: String,
    /// `In-Reply-To:` header value, without angle brackets, or empty when
    /// the message is not a reply.
    pub in_reply_to: String,
    /// Store-defined thread identifier, stable across all messages in the same
    /// conversation.
    pub thread_id: String,
    /// Owner's full address (used to read the raw bytes from a store).
    pub user_address: String,
    /// Optional pre-extracted plain-text snippet, used for `preview`.
    pub new_content: Option<String>,
    /// Implementation-defined opaque id used by [`crate::store::MailStore::read_message_raw`]
    /// to locate the on-disk / blob copy of the message.
    pub blob_id: String,
}

/// Parsed message body parts as returned by [`crate::store::MailStore::parse_message`].
#[derive(Debug, Clone, Default)]
pub struct ParsedBody {
    /// Decoded `text/plain` body, if any.
    pub text: Option<String>,
    /// Decoded `text/html` body, if any.
    pub html: Option<String>,
    /// Attachment metadata (no body bytes — see [`Attachment`]).
    pub attachments: Vec<Attachment>,
}

/// Attachment metadata exposed via JMAP's `Email/get` `attachments` property.
///
/// Body bytes are **not** included; if a client needs them it has to use a
/// separate blob/download endpoint (out of scope for this crate).
#[derive(Debug, Clone)]
pub struct Attachment {
    /// Suggested filename from the `Content-Disposition` header.
    pub filename: String,
    /// MIME type from the `Content-Type` header.
    pub content_type: String,
    /// Size in bytes of the (decoded) attachment payload.
    pub size: u32,
}

/// Result of submitting a previously-stored email through the outbound queue.
#[derive(Debug, Clone)]
pub struct SubmissionResult {
    /// `true` when the message was successfully queued for delivery.
    pub success: bool,
    /// Optional human-readable explanation. On failure this typically carries
    /// the reason; on success it's `None`.
    pub message: Option<String>,
}

/// IMAP `\Seen` flag — message has been viewed.
pub const FLAG_SEEN: u32 = 0b0000_0001;
/// IMAP `\Answered` flag — a reply has been sent.
pub const FLAG_ANSWERED: u32 = 0b0000_0010;
/// IMAP `\Flagged` flag — user-marked for follow-up.
pub const FLAG_FLAGGED: u32 = 0b0000_0100;
/// IMAP `\Deleted` flag — marked for purge on EXPUNGE.
pub const FLAG_DELETED: u32 = 0b0000_1000;
/// IMAP `\Draft` flag — composed but not yet sent.
pub const FLAG_DRAFT: u32 = 0b0001_0000;
