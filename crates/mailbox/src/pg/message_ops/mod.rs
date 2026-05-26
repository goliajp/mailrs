//! Per-message operations on [`crate::pg::PgMailboxStore`].
//!
//! Sub-modules:
//! - [`index`] — single-message and batch insertion paths
//!   (`index_message`, `index_messages_batch`, `append_message`).
//! - [`read`] — lookup/list queries (`list_messages`, `get_message`,
//!   `query_messages`, `count_*`, `find_message_*`,
//!   `get_message_id_by_maildir`, `get_invite_methods`).
//! - [`mutate`] — state changes (`expunge`, `copy_message`,
//!   `move_message`, `update_message_content`, `update_bimi_logo`,
//!   `update_invite_payload`).

mod index;
mod mutate;
mod read;

/// A single delivery's metadata as input to [`crate::pg::PgMailboxStore::index_messages_batch`].
///
/// Borrowed-only — the batch routine binds straight into the
/// multi-row INSERT without taking ownership.
#[derive(Debug, Clone)]
pub struct IndexRecord<'a> {
    /// `local@domain` recipient address. Used to look up the mailbox row.
    pub user: &'a str,
    /// Mailbox folder name (e.g. `"INBOX"`).
    pub mailbox_name: &'a str,
    /// Maildir filename identifier already returned by `Maildir::deliver`.
    pub maildir_id: &'a str,
    /// Decoded `From:` value (header-extracted by the caller).
    pub sender: &'a str,
    /// Decoded `To:` value.
    pub recipients: &'a str,
    /// Decoded `Subject:` value.
    pub subject: &'a str,
    /// Raw message byte length.
    pub size: u32,
    /// Unix timestamp (seconds) used as both `date_epoch` and `internal_date`.
    pub now: i64,
    /// RFC 5322 `Message-ID` (already canonicalised, no angle brackets).
    pub message_id: &'a str,
    /// RFC 5322 `In-Reply-To` first id (empty when absent).
    pub in_reply_to: &'a str,
    /// Thread id resolved by the caller via [`crate::threading::resolve_thread_id`].
    pub thread_id: &'a str,
}
