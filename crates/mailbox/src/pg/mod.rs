//! PostgreSQL reference implementation of [`crate::MailboxStore`](crate::store::MailboxStore).
//!
//! [`PgMailboxStore`] satisfies the portable [`crate::MailboxStore`] trait AND exposes
//! a set of mailrs-specific inherent methods (PG-EXT) for product features that
//! are not part of the portable contract: thread-level UI state (pin / archive
//! / snooze), email-analysis annotations, contact tracking, semantic search,
//! HTML / preview content projections.
//!
//! Programs that want to be store-agnostic should program against
//! [`&dyn MailboxStore`](crate::store::MailboxStore). Programs that need the
//! mailrs-specific features take `&PgMailboxStore` directly. README documents
//! the split.

mod analysis_ops;
mod attachment_ops;
mod contact_ops;
mod flag_ops;
pub(crate) mod helpers;
mod mailbox_ops;
mod message_ops;
mod search_ops;
mod thread_ops;
mod trait_impl;
mod usage_ops;

// Backend selection: PostgreSQL by default, spg-embedded behind the
// `spg` feature. One pool type in the public API; the concrete driver
// is a build-time decision.
/// The sqlx `Database` type of the active backend.
#[cfg(not(feature = "spg"))]
pub type BackendDb = sqlx::Postgres;
/// The sqlx `Database` type of the active backend.
#[cfg(feature = "spg")]
pub type BackendDb = spg_sqlx::Spg;

/// Connection pool of the active backend.
#[cfg(not(feature = "spg"))]
pub type BackendPool = sqlx::PgPool;
/// Connection pool of the active backend.
#[cfg(feature = "spg")]
pub type BackendPool = spg_sqlx::SpgPool;

/// Row type of the active backend.
#[cfg(not(feature = "spg"))]
pub type BackendRow = sqlx::postgres::PgRow;
/// Row type of the active backend.
#[cfg(feature = "spg")]
pub type BackendRow = spg_sqlx::SpgRow;

/// PostgreSQL-backed mailbox metadata store.
///
/// Wraps a [`BackendPool`] and implements [`crate::MailboxStore`](crate::store::MailboxStore)
/// plus a number of mailrs-specific inherent methods. See the module docs for
/// the trait / PG-EXT distinction.
pub struct PgMailboxStore {
    pub(crate) pool: BackendPool,
}

impl PgMailboxStore {
    /// Construct from an existing connected pool. The caller owns the pool
    /// lifecycle; `PgMailboxStore` does not close it on drop. Schema setup
    /// (running `init-schema.sql`) is the caller's responsibility.
    pub fn new(pool: BackendPool) -> Self {
        Self { pool }
    }

    /// Borrow the underlying connection pool. Useful when downstream code
    /// wants to run additional queries against the same connection.
    pub fn pool(&self) -> &BackendPool {
        &self.pool
    }
}

// Re-export PG-EXT public types so callers reach them via
// `mailrs_mailbox::pg::{ContactInfo, EmailAnalysisInput}`.
pub use crate::pg::analysis_ops::EmailAnalysisInput;
pub use crate::pg::contact_ops::ContactInfo;
pub use crate::pg::message_ops::IndexRecord;

// NOTE: `impl MailboxStore for PgMailboxStore` is added in stage 2b together
// with server adaptation. Keeping them in lockstep avoids the complexity of
// maintaining both MessageMeta and Message conversion paths during 2a.
