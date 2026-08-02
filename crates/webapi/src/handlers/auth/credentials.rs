//! Re-authentication and password change — the endpoints that ask for
//! a credential the caller already has.

//! `/api/auth/*` REST handlers.

use std::sync::Arc;

use argon2::Argon2;
use axum::{
    Json,
    extract::{Extension, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use password_hash::{PasswordHash, PasswordVerifier};
use serde::Deserialize;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Deserialize)]
pub struct VerifyRequest {
    pub address: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct VerifyTotpRequest {
    pub address: String,
    pub code: String,
}

fn perm_check_internal_rpc(perms: &[String]) -> bool {
    perms.iter().any(|p| p == "internal.rpc" || p == "*")
}

/// POST /api/auth/verify — password verification without session
/// creation (internal RPC only). Same wire shape as monolith.
pub async fn verify_credentials(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(caller)): Extension<AuthedUser>,
    Json(req): Json<VerifyRequest>,
) -> Response {
    let perms = state
        .core
        .effective_permissions(&caller)
        .await
        .map(|p| p.permissions)
        .unwrap_or_default();
    if !perm_check_internal_rpc(&perms) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "permission denied"})),
        )
            .into_response();
    }
    if req.address.len() > 256 || req.password.len() > 1024 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid input length"})),
        )
            .into_response();
    }
    let acct = match state.core.get_account_with_hash(&req.address).await {
        Ok(a) => a,
        Err(_) => {
            return (
                StatusCode::OK,
                Json(serde_json::json!({"valid": false, "reason": "account_not_found"})),
            )
                .into_response();
        }
    };
    let hash = acct.password_hash.as_deref().unwrap_or("");
    let valid = if hash.is_empty() {
        false
    } else if let Ok(parsed) = PasswordHash::new(hash) {
        Argon2::default()
            .verify_password(req.password.as_bytes(), &parsed)
            .is_ok()
    } else {
        hash == req.password
    };
    if !valid {
        return (
            StatusCode::OK,
            Json(serde_json::json!({"valid": false, "reason": "invalid_password"})),
        )
            .into_response();
    }
    let totp_key = format!("totp:{}", req.address);
    let totp_required = crate::handlers::kevy_util::with_kevy(move |c| {
        c.hget(totp_key.as_bytes(), b"enabled")
            .map_err(std::io::Error::other)
    })
    .ok()
    .flatten()
    .map(|v| v == b"1")
    .unwrap_or(false);
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "valid": true,
            "display_name": acct.public.display_name,
            "domain": acct.public.address.split_once('@').map(|(_, d)| d).unwrap_or(""),
            "totp_required": totp_required,
        })),
    )
        .into_response()
}

/// POST /api/auth/verify-totp — internal-rpc TOTP code check (used by
/// external IdP integrations to defer 2FA to mailrs).
pub async fn verify_totp(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(caller)): Extension<AuthedUser>,
    Json(req): Json<VerifyTotpRequest>,
) -> Response {
    let perms = state
        .core
        .effective_permissions(&caller)
        .await
        .map(|p| p.permissions)
        .unwrap_or_default();
    if !perm_check_internal_rpc(&perms) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "permission denied"})),
        )
            .into_response();
    }
    if req.address.len() > 256 || req.code.len() > 32 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid input length"})),
        )
            .into_response();
    }
    let key = format!("totp:{}", req.address);
    let key_r = key.clone();
    let secret = match crate::handlers::kevy_util::with_kevy(move |c| {
        c.hget(key_r.as_bytes(), b"secret")
            .map_err(std::io::Error::other)
    }) {
        Ok(Some(v)) => String::from_utf8(v).unwrap_or_default(),
        _ => {
            return (
                StatusCode::OK,
                Json(serde_json::json!({"valid": false, "reason": "totp_not_configured"})),
            )
                .into_response();
        }
    };
    let enabled_key = key.clone();
    let enabled = crate::handlers::kevy_util::with_kevy(move |c| {
        c.hget(enabled_key.as_bytes(), b"enabled")
            .map_err(std::io::Error::other)
    })
    .ok()
    .flatten()
    .map(|v| v == b"1")
    .unwrap_or(false);
    if !enabled {
        return (
            StatusCode::OK,
            Json(serde_json::json!({"valid": false, "reason": "totp_not_enabled"})),
        )
            .into_response();
    }
    let code_valid = crate::handlers::totp_util::verify_code(&secret, &req.code);
    let recovery_valid = if !code_valid {
        // v2 Stage B.1: recovery codes are one-shot — consolidate the
        // hget + conditional hset onto a single kevy connection so the
        // read + write happen back-to-back (µs window instead of two
        // TCP round-trips).
        let rc_key = key.clone();
        let code_owned = req.code.clone();
        crate::handlers::kevy_util::with_kevy(move |c| {
            let recovery_str = c
                .hget(rc_key.as_bytes(), b"recovery_codes")
                .map_err(std::io::Error::other)?
                .and_then(|v| String::from_utf8(v).ok())
                .unwrap_or_default();
            let mut codes: Vec<&str> = recovery_str.split(',').filter(|s| !s.is_empty()).collect();
            let Some(idx) = codes.iter().position(|c| *c == code_owned.as_str()) else {
                return Ok(false);
            };
            codes.remove(idx);
            let joined = codes.join(",");
            c.hset(
                rc_key.as_bytes(),
                &[(b"recovery_codes" as &[u8], joined.as_bytes())],
            )
            .map_err(std::io::Error::other)?;
            Ok(true)
        })
        .unwrap_or(false)
    } else {
        false
    };
    if code_valid || recovery_valid {
        (StatusCode::OK, Json(serde_json::json!({"valid": true}))).into_response()
    } else {
        (
            StatusCode::OK,
            Json(serde_json::json!({"valid": false, "reason": "invalid_code"})),
        )
            .into_response()
    }
}

/// POST /api/auth/change-password — verify the current password
/// against the account's stored argon2 hash, then re-hash the new
/// password and patch the account blob. Requires an authenticated
/// session (address comes from the middleware).
pub async fn change_password(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(address)): Extension<AuthedUser>,
    Json(req): Json<ChangePasswordRequest>,
) -> Response {
    use argon2::password_hash::{PasswordHasher, SaltString, rand_core::OsRng as ArgonRng};
    if req.current_password.is_empty() || req.new_password.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "current and new password are required"})),
        )
            .into_response();
    }
    if req.new_password.len() < 8 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "new password must be at least 8 characters"})),
        )
            .into_response();
    }

    // Verify current password via fastcore RPC (same shape as login).
    let acct = match state.core.get_account_with_hash(&address).await {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!(err = %e, "change_password: get_account_with_hash failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let hash = match acct.password_hash.as_deref() {
        Some(h) if !h.is_empty() => h,
        _ => return StatusCode::UNAUTHORIZED.into_response(),
    };
    let parsed = match PasswordHash::new(hash) {
        Ok(p) => p,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if Argon2::default()
        .verify_password(req.current_password.as_bytes(), &parsed)
        .is_err()
    {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "current password is incorrect"})),
        )
            .into_response();
    }

    // Hash new password.
    let salt = SaltString::generate(&mut ArgonRng);
    let new_hash = match Argon2::default().hash_password(req.new_password.as_bytes(), &salt) {
        Ok(h) => h.to_string(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    // Delegate the hash write to fastcore, which owns the embedded
    // kevy that login reads from. Writing to network kevy here (the
    // old code) landed on a store fastcore never consults.
    let req = mailrs_core_api::method::admin::SetPasswordRequest {
        password_hash: new_hash,
    };
    if let Err(e) = state.core.set_account_password(&address, &req).await {
        tracing::warn!(err = %e, %address, "set_account_password failed");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // Revoke all outstanding sessions for this address — the address
    // itself owns the session:by_addr:<address> set of tokens.
    let addr_c = address.clone();
    let _ = crate::handlers::kevy_util::with_kevy(move |c| {
        let idx = format!("session:by_addr:{addr_c}");
        let tokens = c.smembers(idx.as_bytes()).unwrap_or_default();
        for t in tokens {
            let key = format!("session:{}", String::from_utf8_lossy(&t));
            let _ = c.del(&[key.as_bytes()]);
        }
        let _ = c.del(&[idx.as_bytes()]);
        Ok(())
    });
    (StatusCode::OK, Json(serde_json::json!({"success": true}))).into_response()
}
