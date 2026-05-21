//! The [`MailboxStore`] trait — the abstraction every mailbox metadata
//! backend implements.
//!
//! Any Rust mail server (IMAP, JMAP, custom) that needs to persist mailbox
//! and message metadata can program against this trait. The PostgreSQL
//! reference implementation lives in [`crate::pg::PgMailboxStore`]; an
//! in-memory implementation suitable for tests lives in
//! [`crate::fixtures::InMemoryMailboxStore`].
//!
//! The trait is intentionally narrow — IMAP + JMAP primitives only. Product
//! features like thread-level UI state, content projections, and analytics
//! live as inherent methods on the concrete store impls, NOT on the trait.

use async_trait::async_trait;

use crate::types::{
    FlagOp, Inserted, InsertMessage, Mailbox, MailboxStatus, Message, QueryFilter,
};

/// Opaque store error returned by every trait method. Implementations map
/// their own error types (e.g. `sqlx::Error`) into a boxed `dyn Error` at the
/// trait boundary so downstream consumers don't take a transitive dependency
/// on any specific backend.
pub type StoreError = Box<dyn std::error::Error + Send + Sync>;

/// Mailbox metadata storage abstraction.
///
/// Every method is async and object-safe; downstream code can hold
/// `&dyn MailboxStore` or `Arc<dyn MailboxStore>` and stay backend-agnostic.
///
/// ## Contract notes
///
/// - [`insert_message`](Self::insert_message) must allocate `uid` and bump
///   `uidnext` + `highest_modseq` atomically — consumers rely on per-mailbox
///   UID monotonicity.
/// - [`messages_changed_since`](Self::messages_changed_since) must return
///   every message whose `modseq` is strictly greater than the parameter,
///   in `modseq`-ascending order, so CONDSTORE clients can paginate.
/// - [`mailbox_status`](Self::mailbox_status)`.recent` is best-effort. RFC
///   3501 defined `\Recent` as per-session; servers that don't track it
///   should return 0.
/// - [`query_messages`](Self::query_messages)`.text` matches case-insensitively
///   across sender, recipients, and subject. Body-text search is out of
///   scope — different backends index bodies differently.
/// - [`expunge`](Self::expunge) permanently removes every message with the
///   `FLAG_DELETED` bit set, returning the removed UIDs in ascending order.
#[async_trait]
pub trait MailboxStore: Send + Sync {
    // ===== Mailbox CRUD =====

    /// Create a mailbox. Returns the newly-created (or already-existing)
    /// mailbox metadata. Idempotent: if a mailbox with the same name
    /// already exists, returns that mailbox unchanged.
    async fn create_mailbox(&self, user: &str, name: &str) -> Result<Mailbox, StoreError>;

    /// Delete a mailbox by name. Returns `true` if a row was removed,
    /// `false` if no mailbox with that name existed.
    async fn delete_mailbox(&self, user: &str, name: &str) -> Result<bool, StoreError>;

    /// Rename a mailbox.
    async fn rename_mailbox(&self, user: &str, from: &str, to: &str) -> Result<(), StoreError>;

    /// List every mailbox owned by `user`.
    async fn list_mailboxes(&self, user: &str) -> Result<Vec<Mailbox>, StoreError>;

    /// Look up a mailbox by `(user, name)`. Returns `Ok(None)` when not found.
    async fn get_mailbox(&self, user: &str, name: &str) -> Result<Option<Mailbox>, StoreError>;

    /// Look up a mailbox by its store-native id.
    async fn get_mailbox_by_id(&self, id: i64) -> Result<Option<Mailbox>, StoreError>;

    /// Return total / unread / recent message counts for a mailbox.
    async fn mailbox_status(&self, mailbox_id: i64) -> Result<MailboxStatus, StoreError>;

    // ===== Message CRUD =====

    /// Insert a new message into a mailbox. Allocates UID atomically with
    /// `uidnext` and `highest_modseq` bumps. Returns the inserted message's
    /// db id, allocated uid, and resulting modseq.
    async fn insert_message(&self, input: InsertMessage<'_>) -> Result<Inserted, StoreError>;

    /// Look up a message by `(mailbox_id, uid)`. Returns `Ok(None)` when
    /// not found.
    async fn get_message_by_uid(
        &self,
        mailbox_id: i64,
        uid: u32,
    ) -> Result<Option<Message>, StoreError>;

    /// Look up a message by its store-native id.
    async fn get_message(&self, id: i64) -> Result<Option<Message>, StoreError>;

    /// Look up a message by RFC 5322 `Message-ID` header within `user`'s
    /// mailboxes. Used by threading reconstruction.
    async fn find_by_message_id(
        &self,
        user: &str,
        message_id: &str,
    ) -> Result<Option<Message>, StoreError>;

    /// Copy a message between mailboxes (IMAP COPY / JMAP add mailbox to
    /// `mailboxIds`). Returns the new UID in the destination mailbox.
    async fn copy_message(
        &self,
        src_mailbox: i64,
        uid: u32,
        dst_mailbox: i64,
    ) -> Result<u32, StoreError>;

    /// Move a message between mailboxes (RFC 6851 IMAP MOVE / JMAP replace
    /// mailbox in `mailboxIds`).
    async fn move_message(
        &self,
        src_mailbox: i64,
        uid: u32,
        dst_mailbox: i64,
    ) -> Result<u32, StoreError>;

    /// Permanently remove every message in `mailbox_id` with the
    /// `FLAG_DELETED` bit set. Returns the removed UIDs in ascending order.
    async fn expunge(&self, mailbox_id: i64) -> Result<Vec<u32>, StoreError>;

    // ===== Flags (RFC 3501 §6.4.6, RFC 7162 CONDSTORE) =====

    /// Replace a message's flag bitmask. Returns the new `modseq`.
    async fn set_flags(&self, mailbox_id: i64, uid: u32, flags: u32) -> Result<u64, StoreError>;

    /// OR `flags` into the message's current bitmask.
    async fn add_flags(&self, mailbox_id: i64, uid: u32, flags: u32) -> Result<u64, StoreError>;

    /// AND-NOT `flags` out of the message's current bitmask.
    async fn remove_flags(&self, mailbox_id: i64, uid: u32, flags: u32) -> Result<u64, StoreError>;

    /// RFC 7162 CONDSTORE compare-and-swap: apply the flag operation only
    /// if the message's current `modseq` is less than or equal to
    /// `unchangedsince`. Returns `Ok(Some(new_modseq))` on success, `Ok(None)`
    /// when the precondition failed.
    async fn store_flags_if_unchanged(
        &self,
        mailbox_id: i64,
        uid: u32,
        op: FlagOp,
        flags: u32,
        unchangedsince: u64,
    ) -> Result<Option<u64>, StoreError>;

    // ===== Threads =====

    /// Look up the `thread_id` already assigned to a message by its
    /// RFC 5322 `Message-ID` header. Used by threading reconstruction.
    async fn thread_id_for_message(
        &self,
        user: &str,
        message_id: &str,
    ) -> Result<Option<String>, StoreError>;

    /// Return every message id belonging to a thread, ordered by
    /// `internal_date` ascending (oldest first).
    async fn thread_message_ids(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<Vec<i64>, StoreError>;

    /// Walk the `In-Reply-To` / `References` chain backwards from a message,
    /// returning the chain of parent message db ids from immediate parent
    /// up to thread root.
    async fn thread_references(&self, message_id: i64) -> Result<Vec<i64>, StoreError>;

    // ===== Changes (CONDSTORE / JMAP Email/changes) =====

    /// Return every message in `mailbox_id` whose `modseq` is strictly
    /// greater than `modseq`. Result is ordered by `modseq` ascending so
    /// callers can resume by taking the last seen `modseq` from the result.
    async fn messages_changed_since(
        &self,
        mailbox_id: i64,
        modseq: u64,
    ) -> Result<Vec<Message>, StoreError>;

    // ===== Query / Search =====

    /// Run a `QueryFilter` and return matching messages. Supports
    /// pagination via `position` + `limit`. See [`QueryFilter`].
    async fn query_messages(&self, filter: QueryFilter<'_>) -> Result<Vec<Message>, StoreError>;

    // ===== Quota =====

    /// Total bytes of all messages in all of `user`'s mailboxes.
    async fn user_storage_bytes(&self, user: &str) -> Result<u64, StoreError>;
}
