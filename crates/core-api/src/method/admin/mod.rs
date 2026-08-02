//! Admin endpoints — domain_store CRUD + reconcile / export / audit / system_config + api_keys + webhooks + oauth.
//!
//! Source: `crates/server/src/domain_store/*.rs` (57 fn across 12 files) +
//! `api_key_store.rs` + `webhook/store.rs` + `oidc_store.rs`. See
//! `docs/CURRENT_STATE_FROZEN.md` §0.5.
//!
//! Split by subject on 2026-08-02 — 95 wire types and the 92 route
//! constants that name their endpoints. Re-exported flat: these names
//! **are** the wire contract, so moving one between files must not move
//! it in any caller's path.

mod credentials;
mod directory;
mod ops;
mod permissions;
mod userdata;

pub use credentials::*;
pub use directory::*;
pub use ops::*;
pub use permissions::*;
pub use userdata::*;

#[cfg(test)]
mod tests;
