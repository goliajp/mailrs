//! Adapter: server uses `mailrs-auth-guard` (1.0.0) for per-IP +
//! per-(IP, username) failed-auth tracking. This file is a thin
//! re-export so the existing `crate::inbound::auth_guard::{AuthGuard,
//! AuthCheck, AuthGuardConfig}` call sites resolve unchanged.

pub use mailrs_auth_guard::{AuthCheck, AuthGuard, AuthGuardConfig};
