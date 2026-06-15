//! Adapter: server uses `mailrs-auth-guard` (1.0.0) for per-IP +
//! per-(IP, username) failed-auth tracking. This file is a thin
//! re-export so the existing `crate::inbound::auth_guard::{AuthGuard,
//! AuthCheck, AuthGuardConfig, AuthGuardStore}` call sites resolve
//! unchanged.
//!
//! Session handlers hold the guard as `Arc<dyn AuthGuardStore>` (the
//! async trait surface) so a future shared cross-process backend can
//! drop in without touching the call sites. The bundled
//! [`AuthGuard`] is the in-process impl.

pub use mailrs_auth_guard::{AuthCheck, AuthGuard, AuthGuardConfig, AuthGuardStore};

/// Current wall clock in unix seconds — the `now` the auth-guard
/// surface expects. Kept here so every call site computes it the
/// same way.
pub(crate) fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
