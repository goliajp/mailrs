#![doc = include_str!("../README.md")]

pub mod fixtures;
pub mod pg;
pub mod store;
pub mod threading;
pub mod types;

// Public re-exports for the trait surface and its types.
pub use store::{MailboxStore, StoreError};
pub use types::{
    bitmask_to_maildir_flags, maildir_flags_to_bitmask, ConversationSummary, EmailAnalysisRow,
    FlagAction, FlagOp, InsertMessage, Inserted, Mailbox, MailboxStatus, Message, MessageMeta,
    QueryFilter, FLAG_ANSWERED, FLAG_DELETED, FLAG_DRAFT, FLAG_FLAGGED, FLAG_RECENT, FLAG_SEEN,
};

// Public re-exports for the PG reference implementation. Programs that
// want to stay backend-agnostic should depend on the trait via
// `&dyn MailboxStore` and avoid touching the PG-specific re-exports below.
pub use pg::{EmailAnalysisInput, PgMailboxStore};

/// Back-compat alias for the legacy struct name. Prefer [`PgMailboxStore`]
/// in new code. Will be removed in 2.0.
#[deprecated(
    since = "1.0.0",
    note = "renamed to `PgMailboxStore` to make room for the new `MailboxStore` trait"
)]
pub type LegacyMailboxStore = PgMailboxStore;
