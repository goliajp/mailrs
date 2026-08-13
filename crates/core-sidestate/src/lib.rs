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

/// Where an account's Sieve script lives in the shared network store.
///
/// One spelling, because there were five: this handler family, fastcore's
/// ManageSieve, fastcore's delivery-time evaluator, webapi's admin save, and
/// the SQL core's `sieve_store`. All five agreed, which is how the sixth
/// drifts — the same reason `scripts/check-outbound-keys.sh` exists, and it
/// found eight more hand-spelled queue keys the day it was written.
///
/// A key with one definition cannot disagree with itself. Callers in other
/// crates import this rather than formatting their own.
pub fn sieve_key(address: &str) -> String {
    format!("sieve:{address}")
}

/// A core that can open a connection to the shared network kevy. `None`
/// means no network kevy is configured (tests / degraded) — handlers then
/// serve empty results rather than erroring.
pub trait NetKevy: Send + Sync + 'static {
    fn net_conn(&self) -> Option<kevy_client::Connection>;
}

pub mod families;
