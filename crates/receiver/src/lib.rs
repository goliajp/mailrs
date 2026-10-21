//! mailrs-receiver — the SMTP receiving process.
//!
//! It accepts mail, checks each recipient against the core, and writes the
//! message to the spool; delivery is the spool drain's job in whichever core
//! runs. The traits it depends on (the account layer, connection metrics)
//! are free of SQL and kevy-embedded, so the process carries no storage
//! engine of its own.

mod account_store;
pub mod config;
mod conn_metrics;
mod credential_store;
mod kevy_dmarc;
mod recipient_check;
mod run;

pub use account_store::{AccountStore, AccountStoreError};
pub use conn_metrics::ConnectionMetrics;
pub use credential_store::{CoreCredentialStore, CredentialStore};
pub use listeners::Receiver;
pub use recipient_check::RecipientCheck;
pub use run::{run, start};

// The inbound receiving pipeline + anti subsystems + greylist snapshot
// loaders (S5.3). The spg-bound PG loader half of greylist_local and the
// kevy-network adapters (inbound::kevy_backends) stay in the server crate.
pub mod greylist_local;
pub mod greylist_local_sync;
pub mod greylist_sync;
pub mod inbound;
pub mod kevy_net;
pub mod kevy_notify;
pub mod listeners;
pub mod runtime_settings;
pub mod smtp_session;
pub mod spam_lists;
pub mod stats;
