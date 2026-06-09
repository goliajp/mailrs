//! Admin REST handlers, split by resource group.
//!
//! Each sub-module owns a small thematic set of handlers (domains, accounts,
//! aliases, quota, sieve, etc.) plus its own request / response DTOs. Cross-cutting
//! helpers (`require_permission`, default fns, audit-target validation) live here in
//! `mod.rs` so all sub-modules can `use super::*` to pick them up.
//!
//! `web/mod.rs` keeps using handlers as `admin::list_domains`, `admin::add_account`,
//! etc. — the `pub(super) use {domains::*, accounts::*, ...}` block below makes
//! every handler visible at the `admin::` path with no call-site changes.

use axum::Json;
use axum::http::StatusCode;

pub(super) use super::{
    ApiResult, AuthUser, MAX_ADMIN_FIELD_LEN, MAX_PATH_LEN, MAX_SIEVE_SCRIPT_LEN, WebState,
    clamp_limit, classify_email, conversations, default_limit,
};

pub mod account_groups;
pub mod accounts;
pub mod aliases;
pub mod apps;
pub mod audit;
pub mod cache;
pub mod domains;
pub mod email_groups;
pub mod greylist_local;
pub mod groups;
pub mod health;
pub mod oauth;
pub mod permissions;
pub mod policy;
pub mod queue;
pub mod quota;
pub mod sieve;

pub(super) use account_groups::*;
pub(super) use accounts::*;
pub(super) use aliases::*;
pub(super) use apps::*;
pub(super) use audit::*;
pub(super) use cache::*;
pub(super) use domains::*;
pub(super) use email_groups::*;
pub(super) use groups::*;
pub(super) use health::*;
pub(super) use oauth::*;
pub(super) use permissions::*;
pub(super) use policy::*;
pub(super) use queue::*;
pub(super) use quota::*;
pub(super) use sieve::*;

/// helper: check if user has a required permission, return error response if not
pub(super) fn require_permission(
    permissions: &crate::permission::EffectivePermissions,
    perm: &str,
) -> Option<Json<ApiResult>> {
    if permissions.has(perm) {
        None
    } else {
        Some(Json(ApiResult {
            success: false,
            message: Some("insufficient permissions".into()),
        }))
    }
}

pub(super) fn default_alias_type() -> String {
    "alias".into()
}

pub(super) fn default_audit_limit() -> i64 {
    100
}

pub(super) fn default_export_limit() -> i64 {
    1000
}

pub(super) fn default_oauth_scopes() -> String {
    "openid email profile".into()
}

/// validate that the auditor has admin.impersonate and the target user is in accessible domains
pub(super) fn validate_audit_target(
    target_user: &str,
    permissions: &crate::permission::EffectivePermissions,
) -> Result<(), (StatusCode, Json<ApiResult>)> {
    if !permissions.has("admin.impersonate") {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ApiResult {
                success: false,
                message: Some("insufficient permissions".into()),
            }),
        ));
    }
    let domain = target_user.split_once('@').map(|(_, d)| d).unwrap_or("");
    if domain.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiResult {
                success: false,
                message: Some("invalid target user address".into()),
            }),
        ));
    }
    let accessible = permissions.accessible_domains();
    if !permissions.is_super() && !accessible.iter().any(|d| d == domain) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ApiResult {
                success: false,
                message: Some("target user not in accessible domains".into()),
            }),
        ));
    }
    Ok(())
}
