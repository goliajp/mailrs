//! Cement seam over the [`MessageStore`] backend for delivered mail.
//!
//! The receiver/core split routes every *single-message* local
//! delivery (IMAP APPEND, web compose → INBOX/Sent) through the
//! [`MessageStore`] trait so the storage backend is swappable
//! (maildir today, an object-store backend at P7). The SMTP fast
//! path is deliberately **not** here: it keeps using the maildir
//! group-commit `DeliveryExecutor`, which is a maildir-specific
//! fsync-batching optimisation that lives behind this seam, not in
//! front of it.
//!
//! [`default_store`] is the single construction point — P7 swaps the
//! backend by editing only this function.

use std::sync::Arc;

use mailrs_mailbox::PgMailboxStore;
pub use mailrs_message_store::{MaildirStore, MessageId, MessageStore};

/// The configured delivered-message backend. `MaildirStore` today;
/// P7 swaps to an object-store backend here (driven by config). This
/// is the only place a concrete [`MessageStore`] is named, so the
/// seam stays a one-line change.
pub fn default_store() -> Arc<dyn MessageStore> {
    Arc::new(MaildirStore)
}

/// Deliver one message through `store`, then index it in `mailbox`.
///
/// This is the cement glue for single-message local delivery: the
/// write goes over the [`MessageStore`] trait (backend-swappable),
/// the metadata indexing stays in the mailbox stone. Returns the
/// assigned `(uid, maildir_id)` — same shape as the legacy
/// `PgMailboxStore::append_message` it replaces at the call sites.
// the two backends plus the six delivery params mirror exactly what the
// call site already holds; wrapping them in a struct adds a noisy
// round-trip without simplifying anything.
#[allow(clippy::too_many_arguments)]
pub async fn deliver_and_index(
    store: &dyn MessageStore,
    mailbox: &PgMailboxStore,
    user: &str,
    mailbox_name: &str,
    maildir_root: &str,
    data: &[u8],
    flags: u32,
    now: i64,
) -> Result<(u32, String), String> {
    let (local, domain) = user
        .split_once('@')
        .ok_or_else(|| "invalid user address".to_string())?;
    let path = format!("{maildir_root}/{domain}/{local}");

    let ids = store
        .deliver_batch(&path, &[data])
        .await
        .map_err(|e| format!("failed to deliver: {e}"))?;
    let msg_id = ids
        .into_iter()
        .next()
        .ok_or_else(|| "delivery returned no message id".to_string())?
        .0;

    let uid = mailbox
        .index_delivered(user, mailbox_name, &msg_id, data, flags, now)
        .await?;

    Ok((uid, msg_id))
}
