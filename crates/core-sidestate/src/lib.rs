//! Shared side-state handlers for the two mailrs cores.
//!
//! The side-state families (drafts / signatures / templates / reactions /
//! webhooks / audit / contacts / analysis / outbound / groups / api-keys /
//! sieve) live in the INDEPENDENT network kevy — they are not part of the
//! switchable mail store. Both cores serve their contract routes; to make
//! that behaviour BYTE-IDENTICAL (not just via webapi, which bypasses the
//! core), both mount these ONE generic implementation rather than each
//! keeping its own copy.
//!
//! A core provides network-kevy access by implementing [`NetKevy`]; every
//! handler here is generic over `S: NetKevy` and is mounted by both
//! `mailrs-fastcore` and the pg-core (`mailrs-server --features core-rpc`).

/// A core that can open a connection to the shared network kevy. `None`
/// means no network kevy is configured (tests / degraded) — handlers then
/// serve empty results rather than erroring.
pub trait NetKevy: Send + Sync + 'static {
    fn net_conn(&self) -> Option<kevy_client::Connection>;
}

pub mod families;
