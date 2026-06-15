//! mailrs-receiver — the SMTP receiving path's *ports*.
//!
//! This crate holds the traits the receiver depends on for the account
//! layer, quota, and connection metrics, plus the pure
//! [`ResolvedRecipient`] data type. They are free of spg / kevy so the
//! receiver can be built (and eventually split into its own process)
//! without binding the stateful core's storage engines.
//!
//! The spg/kevy-backed *adapters* (impls for `DomainStore`,
//! `PgMailboxStore`, `WebState`) live in the server core — ports here,
//! adapters there.

mod account_store;
mod conn_metrics;
mod quota_store;
mod recipient;

pub use account_store::{AccountStore, AccountStoreError};
pub use conn_metrics::ConnectionMetrics;
pub use quota_store::QuotaStore;
pub use recipient::ResolvedRecipient;
