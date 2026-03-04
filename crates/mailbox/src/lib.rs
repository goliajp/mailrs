pub mod store;
pub mod threading;
pub mod types;

pub use store::MailboxStore;
pub use types::{
    ConversationSummary, EmailAnalysisRow, FlagAction, Mailbox, MessageMeta,
    FLAG_ANSWERED, FLAG_DELETED, FLAG_DRAFT, FLAG_FLAGGED, FLAG_RECENT, FLAG_SEEN,
    bitmask_to_maildir_flags, maildir_flags_to_bitmask,
};
