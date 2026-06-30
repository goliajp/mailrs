//! Admin handlers — Phase 2.2 priority subset for webapi split unblock.
//!
//! Covers the auth hot-path:
//! - GET /v1/admin/api-keys/by-prefix/{prefix}     — every API-key authed req
//! - POST /v1/admin/api-keys/{id}/touch            — async update_last_used
//! - GET /v1/admin/accounts/{address}/effective-permissions  — every authed req
//! - GET /v1/admin/accounts/{address}/credentials  — SMTP/IMAP/POP3 AUTH
//! - GET /v1/admin/accounts                        — list (admin web UI)
//! - GET /v1/admin/accounts/{address}              — get one (admin web UI)
//!
//! Remaining 50+ admin endpoints land in subsequent loops.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::TimeZone;

use mailrs_core_api::method::admin as wire;

use crate::core_rpc::CoreRpcState;

// ── api keys ────────────────────────────────────────────────────────

/// GET /v1/admin/api-keys/by-prefix/{prefix}
///
/// Returns the API key row for AUTH verify. `key_hash` field is `#[serde(skip)]`
/// in `ApiKeyWire` — caller hashes the candidate full_key and compares
/// server-side. Returning the hash via wire would be a security regression.
///
/// Note: today this handler is incomplete — the wire type intentionally
/// elides `key_hash` for security, but server-side cement needs the hash to
/// argon2-verify. Until checklist 2.5 wraps internal auth, we expose `key_hash`
/// only when the caller is on the internal network (assumed via
/// `core-rpc` channel having `MAILRS_CORE_API_SECRET` bearer auth).
pub async fn get_api_key_by_prefix(
    State(state): State<Arc<CoreRpcState>>,
    Path(prefix): Path<String>,
) -> Result<Json<wire::ApiKeyWire>, StatusCode> {
    let row = crate::api_key_store::get_api_key_by_prefix(&state.pool, &prefix)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "api_key by prefix lookup failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(wire::ApiKeyWire {
        id: row.id,
        prefix: row.prefix,
        full_key: row.full_key,
        key_hash: row.key_hash,
        account_address: row.account_address,
        name: row.name,
        expires_at: row.expires_at.map(|d| d.timestamp()),
        last_used_at: row.last_used_at.map(|d| d.timestamp()),
        revoked_at: row.revoked_at.map(|d| d.timestamp()),
        created_at: row.created_at.timestamp(),
        app_id: row.app_id,
    }))
}

/// POST /v1/admin/api-keys/{id}/touch
///
/// Update `last_used_at = now()`. Fire-and-forget on caller side; we still
/// wait + log on error so the operator sees if writes are failing.
pub async fn touch_api_key(
    State(state): State<Arc<CoreRpcState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    crate::api_key_store::update_last_used(&state.pool, id).await;
    Ok(StatusCode::NO_CONTENT)
}

// ── effective permissions ───────────────────────────────────────────

/// GET /v1/admin/accounts/{address}/effective-permissions
///
/// Called per authed web request (`/api/auth/me` + middleware). Critical
/// hot path — webapi caches the result in kevy session.
pub async fn effective_permissions(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
) -> Result<Json<wire::EffectivePermissionsResponse>, StatusCode> {
    let perms = state
        .domain
        .load_account_permissions(&address)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, address = %address, "load perms failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(wire::EffectivePermissionsResponse {
        address: address.clone(),
        permissions: perms.permission_list(),
        // Group resolution is left for a separate RPC call to keep this
        // endpoint cheap; webapi can hit /accounts/{address}/groups when it
        // needs the group list (rarely; not part of the auth-me hot path).
        groups: Vec::new(),
        is_super: perms.is_super(),
        send_as: perms.send_as().to_vec(),
    }))
}

// ── accounts (read) ─────────────────────────────────────────────────

/// GET /v1/admin/accounts/{address}/credentials
///
/// SMTP/IMAP/POP3/MgSieve AUTH path. Returns Account public fields + the
/// argon2 password hash. Bearer-secret-only; never expose to webapi public.
pub async fn get_account_with_hash(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
) -> Result<Json<wire::AccountWithHashWire>, StatusCode> {
    let (account, password_hash) = state
        .domain
        .get_account_with_hash(&address)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, address = %address, "get_account_with_hash failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    let public = wire::AccountWire {
        address: account.address,
        domain: account.domain,
        display_name: account.display_name,
        active: account.active,
        created_at: account.created_at,
        quota_bytes: account.quota_bytes,
        recovery_email: if account.recovery_email.is_empty() {
            None
        } else {
            Some(account.recovery_email)
        },
    };
    Ok(Json(wire::AccountWithHashWire {
        public,
        password_hash: Some(password_hash),
    }))
}

/// GET /v1/admin/accounts
pub async fn list_accounts(
    State(state): State<Arc<CoreRpcState>>,
) -> Result<Json<wire::AccountListResponse>, StatusCode> {
    let rows = state.domain.list_accounts().await.map_err(|e| {
        tracing::warn!(error = %e, "list_accounts failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let items = rows
        .into_iter()
        .map(|a| wire::AccountWire {
            address: a.address,
            domain: a.domain,
            display_name: a.display_name,
            active: a.active,
            created_at: a.created_at,
            quota_bytes: a.quota_bytes,
            recovery_email: if a.recovery_email.is_empty() {
                None
            } else {
                Some(a.recovery_email)
            },
        })
        .collect();
    Ok(Json(wire::AccountListResponse { items }))
}

/// GET /v1/admin/accounts/{address}
///
/// Public account info (no hash). Caches via the same DomainStore kevy LRU.
pub async fn get_account(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
) -> Result<Json<wire::AccountWire>, StatusCode> {
    let (account, _) = state
        .domain
        .get_account_with_hash(&address)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, address = %address, "get_account failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(wire::AccountWire {
        address: account.address,
        domain: account.domain,
        display_name: account.display_name,
        active: account.active,
        created_at: account.created_at,
        quota_bytes: account.quota_bytes,
        recovery_email: if account.recovery_email.is_empty() {
            None
        } else {
            Some(account.recovery_email)
        },
    }))
}

// Silence unused imports under future loops:
#[allow(dead_code)]
fn _import_tz() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc.timestamp_opt(0, 0).unwrap()
}
