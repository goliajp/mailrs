pub mod store;
pub mod threading;
pub mod types;

mod analysis_ops;
mod attachment_ops;
mod contact_ops;
mod flag_ops;
mod helpers;
mod mailbox_ops;
mod message_ops;
mod search_ops;
mod thread_ops;
mod usage_ops;

pub use store::MailboxStore;
pub use types::{
    ConversationSummary, EmailAnalysisRow, FlagAction, Mailbox, MessageMeta,
    FLAG_ANSWERED, FLAG_DELETED, FLAG_DRAFT, FLAG_FLAGGED, FLAG_RECENT, FLAG_SEEN,
    bitmask_to_maildir_flags, maildir_flags_to_bitmask,
};
