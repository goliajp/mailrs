//! Admin endpoints — accounts, aliases, domains, groups and effective
//! permissions, the audit log, webhook subscriptions, and the per-user
//! drafts, signatures and templates.
//!
//! Split by subject on 2026-08-02; today 37 wire types and the 20 route
//! constants that name their endpoints. Re-exported flat: these names
//! **are** the wire contract, so moving one between files must not move
//! it in any caller's path.

mod account_purge;
mod credentials;
mod directory;
mod ops;
mod ownerless;
mod permissions;
mod quota;
mod rejudge;
mod usage_drift;
mod userdata;

pub use account_purge::*;
pub use credentials::*;
pub use directory::*;
pub use ops::*;
pub use ownerless::*;
pub use permissions::*;
pub use quota::*;
pub use rejudge::*;
pub use usage_drift::*;
pub use userdata::*;

#[cfg(test)]
mod tests;
