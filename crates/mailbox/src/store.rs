use sqlx::PgPool;

/// mailbox storage backed by PG for metadata and maildir for message bodies
pub struct MailboxStore {
    pub(crate) pool: PgPool,
}

impl MailboxStore {
    /// Borrow the underlying connection pool. Useful when downstream code
    /// wants to run additional queries against the same connection.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

// re-export `ContactInfo` here so existing callers that reference
// `mailrs_mailbox::store::ContactInfo` keep working after the refactor.
pub use crate::contact_ops::ContactInfo;
