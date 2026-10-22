//! Admin endpoints — accounts, aliases, domains, groups and effective
//! permissions, the audit log, webhook subscriptions, and the per-user
//! drafts, signatures and templates.
//!
//! Split by subject on 2026-08-02; today 37 wire types and the 20 route
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
