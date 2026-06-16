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
pub mod config;
mod conn_metrics;
mod quota_store;
mod recipient;
mod run;

pub use account_store::{AccountStore, AccountStoreError};
pub use conn_metrics::ConnectionMetrics;
pub use quota_store::QuotaStore;
pub use recipient::ResolvedRecipient;
pub use run::run;

// The inbound receiving pipeline + anti subsystems + greylist snapshot
// loaders (S5.3). The spg-bound PG loader half of greylist_local and the
// kevy-network adapters (inbound::kevy_backends) stay in the server crate.
pub mod greylist_local;
pub mod greylist_sync;
pub mod inbound;
pub mod kevy_net;
pub mod kevy_notify;
pub mod listeners;
pub mod smtp_session;
