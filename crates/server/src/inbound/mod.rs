//! re-export shim: the inbound receiving pipeline + anti subsystems moved
//! to the `mailrs-receiver` crate (S5.3). Re-exported here so the
//! `crate::inbound::{auth_guard, rate_limit, pipeline, content_scan,
//! stages}` call sites (web / imap / pop3 / managesieve / smtp / bootstrap)
//! stay unchanged.
//!
//! `kevy_backends` stays in the server crate: it bridges the receiver's
//! anti trait surfaces (GreylistBackend / RateLimitStore / AuthGuardStore)
//! to the network kevy client (`crate::kevy_net`), which is server-side
//! infrastructure.

pub use mailrs_receiver::inbound::{auth_guard, content_scan, pipeline, rate_limit, stages};

pub mod kevy_backends;
