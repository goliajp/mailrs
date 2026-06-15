//! Receiver-facing account / recipient port.

use crate::ResolvedRecipient;

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

/// The account-layer lookups the SMTP receiving path needs: recipient
/// resolution, submission auth, and per-recipient policy (sieve / vacation
/// / quota). Implemented in-process by the server's `DomainStore`;
/// abstracted here so the receiver depends only on the trait.
#[async_trait::async_trait]
pub trait AccountStore: Send + Sync {
    /// Resolve a recipient address through aliases / groups / forwards.
    async fn resolve_recipient(&self, address: &str) -> ResolvedRecipient;

    /// The submission password hash for `address`, if the account exists.
    /// The caller verifies the password against it (argon2 or legacy plain).
    async fn password_hash(&self, address: &str) -> Result<Option<String>, AccountStoreError>;

    /// The recipient's sieve script source, if one is configured.
    async fn sieve_script(&self, address: &str) -> Result<Option<String>, AccountStoreError>;

    /// The recipient's quota in bytes, if set (`0` means unlimited).
    async fn quota(&self, address: &str) -> Result<Option<i64>, AccountStoreError>;

    /// Whether a vacation auto-reply should be sent for this
    /// `(recipient, sender, handle)` triple — RFC 5230 dedup over
    /// `period_secs`.
    async fn should_send_vacation_reply(
        &self,
        recipient: &str,
        sender: &str,
        handle: &str,
        period_secs: u64,
    ) -> Result<bool, AccountStoreError>;

    /// Record that a vacation reply was just sent for this triple.
    async fn record_vacation_reply(
        &self,
        recipient: &str,
        sender: &str,
        handle: &str,
        now: i64,
    ) -> Result<(), AccountStoreError>;
}
