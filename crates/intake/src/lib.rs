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

#[cfg(feature = "abort-hook")]
mod abort_hook;
mod deliver;
mod drain;
mod forward;
mod forward_loop;
mod group;
mod mail_settings;

pub mod arrival;
pub mod attachment_text;
pub mod domains;
pub mod dsn;
pub mod effects;
pub mod headers;
pub mod idle_backoff;
pub mod image_prefetch;
pub mod import;
pub mod importance;
pub mod invite;
pub mod live_sync;
pub mod mail_files;
pub mod maildir;
pub mod mock;
pub mod notice;
pub mod outbound;
pub mod push;
pub mod push_policy;
pub mod reports;
pub mod route;
pub mod rules;
pub mod scam_report;
pub mod section_facts;
pub mod sender_ops;
pub mod sent;
pub mod sieve;
pub mod sieve_policy;
pub mod spool;
pub mod unified_push;
pub mod web_push;
pub mod webhook_delivery;
pub mod why;

pub use drain::{Copies, Core, Inbound, drain_once};

#[cfg(test)]
mod arrival_tests;
#[cfg(test)]
mod route_tests;
