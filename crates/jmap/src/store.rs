//! The [`MailStore`] trait: the abstraction layer between JMAP method handlers
//! and any backing store (PostgreSQL + Maildir, IMAP proxy, etc.).
//!
//! Implementations should be cheap to clone or, more commonly, accessed
//! through `&dyn MailStore`. All methods are async and object-safe.

use async_trait::async_trait;

use crate::types::{Mailbox, MailboxCounts, Message, ParsedBody, SubmissionResult};

/// Opaque store error returned to the dispatcher. Handlers convert it into a
/// JMAP `serverFail` method error with the `Display` value as `description`.
pub type StoreError = Box<dyn std::error::Error + Send + Sync>;

#[async_trait]
pub trait MailStore: Send + Sync {
    /// List all mailboxes for `user`. Used by `Mailbox/get` and `Mailbox/query`.
    async fn list_mailboxes(&self, user: &str) -> Result<Vec<Mailbox>, StoreError>;

    /// Return `(total, unread)` for a mailbox. May be best-effort: handlers
    /// fall back to `(0, 0)` on error.
    async fn mailbox_status(&self, mailbox_id: i64) -> Result<MailboxCounts, StoreError>;

    /// List up to `limit` messages in `mailbox_id` starting at `offset`,
    /// ordered however the store deems natural (typically by uid).
    async fn list_messages(
        &self,
        mailbox_id: i64,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<Message>, StoreError>;

    /// Look up a message by its database/primary id. Returns `Ok(None)` when
    /// the id exists but doesn't belong to `user`, or when no such id exists.
    async fn get_message_by_db_id(
        &self,
        user: &str,
        id: i64,
    ) -> Result<Option<Message>, StoreError>;

    /// All messages in `thread_id` for `user`, ordered chronologically.
    async fn list_thread_messages(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<Vec<Message>, StoreError>;

    /// Replace a message's flag bitmask. `mailbox_id` + `uid` identify the row.
    async fn update_flags(
        &self,
        mailbox_id: i64,
        uid: u32,
        flags: u32,
    ) -> Result<(), StoreError>;

    /// OR `flags` into the message's flag bitmask. Used to set `\Deleted`.
    async fn add_flags(
        &self,
        mailbox_id: i64,
        uid: u32,
        flags: u32,
    ) -> Result<(), StoreError>;

    /// Read the raw RFC 5322 bytes for a message. Returns `None` when not
    /// available (e.g. blob missing on disk). Used to populate `bodyValues`,
    /// `textBody`, `htmlBody`, and the submission path.
    async fn read_message_raw(&self, message: &Message) -> Option<Vec<u8>>;

    /// Parse raw bytes into text/html body plus attachment metadata. Pulled
    /// out of [`Self::read_message_raw`] so the same bytes can serve both
    /// `Email/get` rendering and `EmailSubmission/set` outbound delivery.
    fn parse_message(&self, raw: &[u8]) -> ParsedBody;

    /// Submit a previously-stored email through the outbound queue.
    ///
    /// The handler has already resolved the [`Message`] and read its raw bytes
    /// via [`Self::read_message_raw`]; the store implementation is responsible
    /// for queueing / signing / actually sending.
    async fn submit_message(
        &self,
        user: &str,
        message: &Message,
        raw: &[u8],
    ) -> SubmissionResult;
}
