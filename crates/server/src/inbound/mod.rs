//! re-export shim: the inbound receiving pipeline + anti subsystems moved
//! to the `mailrs-receiver` crate (S5.3). Re-exported here so the
//! `crate::inbound::{auth_guard, rate_limit, pipeline, content_scan,
//! stages}` call sites (web / imap / pop3 / managesieve / smtp / bootstrap)
//! stay unchanged.
//!
//! `kevy_backends` (the network kevy anti adapters) moved to mailrs-receiver
//! in P6-S5 so the receiver binary can construct them; re-exported here so
//! `crate::inbound::kevy_backends::*` call sites stay unchanged.

pub use mailrs_receiver::inbound::{
    auth_guard, content_scan, kevy_backends, pipeline, rate_limit, stages,
};
