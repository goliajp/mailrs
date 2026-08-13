//! `/api/admin/*` REST handlers — fastcore-native.
//!
//! - Accounts routes proxy to fastcore RPC (accounts live in kevy).
//! - Aliases / domains / webhooks / audit are stored in the shared
//!   network kevy under `admin:*` keys.
//!
//! Zero spg touch.

use axum::http::StatusCode;

// Split by subject on 2026-08-02. Re-exported so the router keeps naming
// `handlers::admin::…`.
pub(crate) use crate::handlers::admin_audit::*;
pub(crate) use crate::handlers::admin_directory::*;
pub(crate) use crate::handlers::admin_ops::*;

pub(crate) fn map_err(e: mailrs_core_api::error::CoreApiError) -> StatusCode {
    StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

pub(crate) fn hgetall_values(
    c: &mut kevy_client::Connection,
    key: &str,
) -> std::io::Result<Vec<Vec<u8>>> {
    let flat = c.hgetall(key.as_bytes()).map_err(std::io::Error::other)?;
    Ok(flat
        .into_iter()
        .enumerate()
        .filter_map(|(i, v)| if i % 2 == 1 { Some(v) } else { None })
        .collect())
}

// ── accounts (via fastcore RPC) ────────────────────────────────────

// ── aliases (network kevy) ─────────────────────────────────────────

/// Legacy hash key populated by the pre-fastcore-split admin panel.
/// Retained for `sync_aliases_to_fastcore` boot-time mirror that
/// migrates leftover rows out to the canonical alias-store; every
/// runtime read/write now goes through `state.core.list_local_aliases`
/// etc. (see v2.2-fix 2026-07-09).
pub(crate) const ALIAS_KEY: &str = "admin:aliases";

// ── domains (via fastcore RPC) ─────────────────────────────────────

// ── webhooks (network kevy) ────────────────────────────────────────

// ── audit log (network kevy list) ──────────────────────────────────

pub(crate) const AUDIT_KEY: &str = "admin:audit_log";

// ── account extras: PUT / quota / sieve / groups / overrides ─────

// ── domain DNS check ──────────────────────────────────────────────

// ── reconcile-maildir + suppressions + email-groups-members ──────

// ── /api/admin/export — bulk export a user's messages ────────────
