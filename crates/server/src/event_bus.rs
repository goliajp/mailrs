//! The event bus moved to the `mailrs-receiver` crate (the receiving path
//! owns the `SmtpEvent` vocabulary). Re-exported here so existing
//! `crate::event_bus::…` call sites across the server keep resolving, and so
//! the kevy-backed [`EventPublisher`] adapter in `kevy_notify` can implement
//! the (now-foreign) trait for its server-local publisher type.

pub use mailrs_core::event_bus::*;
