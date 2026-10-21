//! Receiver-facing account port.

/// Error from an [`AccountStore`] lookup. Backend details are stringified so
/// the trait stays dyn-compatible and free of spg / sqlx types — the seam a
/// network-backed store can implement without leaking its transport errors.
#[derive(Debug)]
pub enum AccountStoreError {
    /// Backend-specific error (spg, network — stringified).
    Backend(String),
}

impl std::fmt::Display for AccountStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccountStoreError::Backend(e) => write!(f, "account store backend: {e}"),
        }
    }
}

impl std::error::Error for AccountStoreError {}

/// The account lookup the SMTP receiving path needs: the submission password
/// hash. Implemented in-process by the server's domain store; abstracted
/// here so the receiver depends only on the trait.
#[async_trait::async_trait]
pub trait AccountStore: Send + Sync {
    /// The submission password hash for `address`, if the account exists.
    /// The caller verifies the password against it (argon2 or legacy plain).
    async fn password_hash(&self, address: &str) -> Result<Option<String>, AccountStoreError>;
}
