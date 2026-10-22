//! Cement seam over the [`MessageStore`] backend for delivered mail.
//!
//! IMAP reads, APPEND, COPY, MOVE and EXPUNGE reach a body through the
//! [`MessageStore`] trait (`crate::body` says where the body is), so the
//! storage backend is swappable.
//!
//! [`default_store`] is the single construction point — P7 swaps the
//! backend by editing only this function.

use std::sync::Arc;

pub use mailrs_message_store::{MaildirStore, MessageStore};

/// The configured delivered-message backend. `MaildirStore` today;
/// P7 swaps to an object-store backend here (driven by config). This
/// is the only place a concrete [`MessageStore`] is named, so the
/// seam stays a one-line change.
pub fn default_store() -> Arc<dyn MessageStore> {
    Arc::new(MaildirStore)
}
