//! Per-method JMAP handlers (RFC 8621).

pub mod email;
pub mod mailbox;
pub mod submission;
pub mod thread;

pub use email::{handle_email_get, handle_email_query, handle_email_set};
pub use mailbox::{handle_mailbox_get, handle_mailbox_query, mailbox_role};
pub use submission::handle_email_submission_set;
pub use thread::handle_thread_get;
