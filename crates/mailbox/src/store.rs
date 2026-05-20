use sqlx::PgPool;

/// mailbox storage backed by PG for metadata and maildir for message bodies
pub struct MailboxStore {
    pub(crate) pool: PgPool,
}

// re-export `ContactInfo` here so existing callers that reference
// `mailrs_mailbox::store::ContactInfo` keep working after the refactor.
pub use crate::contact_ops::ContactInfo;
