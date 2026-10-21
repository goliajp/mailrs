//! re-export shim: the SMTP receiving session moved to the
//! `mailrs-receiver` crate (S5.4). Re-exported here so the
//! `crate::smtp_session::{ConnectionContext, handle_plain_connection,
//! handle_tls_connection, DeliveredMessage, ProcessTx, ...}` call sites
//! (bootstrap / listeners / test_support) stay unchanged.
//!
//! `post_delivery` + `process_delivered` stay in the server crate: they are
//! the spg/kevy-bound post-delivery *consumer* (PgMailboxStore, the calendar
//! reconcile, the outbound BackendPool). The receiver hands off a plain
//! [`mailrs_receiver::smtp_session::DeliveredMessage`] over the channel; the
//! consumer here owns the stateful deps.

pub use mailrs_receiver::smtp_session::*;

mod post_delivery;
mod process_delivered;

pub(crate) use process_delivered::{ProcessDeps, spawn_process_consumer};
