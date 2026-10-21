//! The receiving half both mailrs cores share.
//!
//! The receiver writes every accepted message to a spool; a core drains it.
//! Two cores do that — the kevy core and the SQL core — and they must
//! deliver alike, so every step that decides something about a message is
//! here, once: which spool files to read and when, where each recipient's
//! copy goes (its mailbox, an SRS bounce, an email group's members, an
//! alias target, the postmaster or abuse fallback), group expansion,
//! mock-address diversion, and the outbound queue for forwards and
//! submissions.
//!
//! A core implements [`route::Directory`] to answer who its recipients
//! are, and [`Core`] to say what happens to a copy that reached one of its
//! mailboxes. Nothing here knows which store is behind either.

mod drain;
mod group;

pub mod domains;
pub mod maildir;
pub mod mock;
pub mod notice;
pub mod outbound;
pub mod route;
pub mod spool;

pub use drain::{Copies, Core, Inbound, drain_once};

#[cfg(test)]
mod route_tests;
