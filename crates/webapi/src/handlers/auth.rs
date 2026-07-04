//! `/api/auth/*` REST handlers.

use std::sync::Arc;

use argon2::Argon2;
use axum::{
    Json,
    extract::{Extension, State},
    http::{StatusCode, header::SET_COOKIE},
    response::{IntoResponse, Response},
};
use password_hash::{PasswordHash, PasswordVerifier};
use rand_core::RngCore;
use serde::{Deserialize, Serialize};

use crate::WebState;
use crate::handlers::conversations::{AuthedDisplayName, AuthedUser};

fn map_err(e: mailrs_core_api::error::CoreApiError) -> StatusCode {
    StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

/// Shape returned by `GET /api/auth/me`. Mirrors the monolith handler
/// in `crates/server/src/web/auth/login.rs:485` so the frontend payload
/// is identical.
#[derive(Debug, Serialize)]
pub struct AuthMeResponse {
    pub address: String,
    pub display_name: String,
    pub permissions: Vec<String>,
    pub accessible_domains: Vec<String>,
    pub send_as: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub address: String,
    pub password: String,
    #[serde(default)]
    pub totp_code: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub address: String,
    pub display_name: String,
    pub permissions: Vec<String>,
    /// Session token — same shape as monolith. Frontend stores this
    /// in the auth store; subsequent authenticated requests send it
    /// as `Authorization: Bearer <token>`.
    pub token: String,
}

/// POST /api/auth/login
///
/// - Resolve the account + argon2 hash via core RPC
/// - Verify password
/// - Generate 32-byte random session token (hex)
/// - Write `session:<token>` to kevy with the SessionInfoWire shape the
///   monolith uses, so either binary can read it
/// - Return 200 with `Set-Cookie: mailrs_session=<token>; HttpOnly; ...`
pub async fn login(State(state): State<Arc<WebState>>, Json(req): Json<LoginRequest>) -> Response {
    // Fastcore-only: kevy-backed account store is the source of truth.
    let acct = match state.fast().get_account_with_hash(&req.address).await {
        Ok(a) => a,
        Err(mailrs_core_api::error::CoreApiError::NotFound(_)) => {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        Err(e) => {
            tracing::warn!(error = %e, "login: get_account_with_hash failed");
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
        .verify_password(req.password.as_bytes(), &parsed)
        .is_err()
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    // TOTP gate — if enrolled, require a 6-digit code (or a
    // recovery code) on the login request; otherwise short-circuit
    // with `{ requires_totp: true }` so the UI can prompt.
    //
    // Kevy failure while checking TOTP must NOT fall through to
    // "session issued" — that would let a kevy blink bypass 2FA.
    // Fail closed with 500 instead.
    let totp_key = format!("totp:{}", req.address);
    let totp_key_r = totp_key.clone();
    let totp_enrolled_secret = match crate::handlers::kevy_util::with_kevy(move |c| {
        Ok((
            c.hget(totp_key_r.as_bytes(), b"secret")?,
            c.hget(totp_key_r.as_bytes(), b"enabled")?,
        ))
    }) {
        Ok(v) => v,
        Err(_) => {
            tracing::warn!(addr = %req.address, "login: kevy TOTP check failed; rejecting to fail-closed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    if let (Some(secret_bytes), Some(en)) = totp_enrolled_secret
        && en == b"1"
    {
        let secret = String::from_utf8(secret_bytes).unwrap_or_default();
        let Some(code) = req.totp_code.as_ref() else {
            return (
                StatusCode::OK,
                Json(serde_json::json!({ "requires_totp": true })),
            )
                .into_response();
        };
        let ok_code = crate::handlers::totp_util::verify_code(&secret, code);
        let ok_recovery = if !ok_code {
            let rc_key = totp_key.clone();
            let recovery_str = crate::handlers::kevy_util::with_kevy(move |c| {
                c.hget(rc_key.as_bytes(), b"recovery_codes")
            })
            .ok()
            .flatten()
            .and_then(|v| String::from_utf8(v).ok())
            .unwrap_or_default();
            let mut codes: Vec<&str> = recovery_str.split(',').filter(|s| !s.is_empty()).collect();
            let pos = codes.iter().position(|c| *c == code);
            if let Some(idx) = pos {
                codes.remove(idx);
                let joined = codes.join(",");
                let write_key = totp_key.clone();
                let _ = crate::handlers::kevy_util::with_kevy(move |c| {
                    c.hset(
                        write_key.as_bytes(),
                        &[(b"recovery_codes" as &[u8], joined.as_bytes())],
                    )?;
                    Ok(())
                });
                true
            } else {
                false
            }
        } else {
            false
        };
        if !ok_code && !ok_recovery {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "invalid TOTP code" })),
            )
                .into_response();
        }
    }

    // Permissions for the login response — fastcore-only.
    let perms = state.fast().effective_permissions(&req.address).await.ok();

    // Generate token + write to kevy in the same shape as the monolith.
    let mut bytes = [0u8; 32];
    rand_core::OsRng.fill_bytes(&mut bytes);
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();

    let empty_strings: Vec<String> = Vec::new();
    let perms_obj = match perms.as_ref() {
        Some(p) => serde_json::json!({
            "permissions": p.permissions,
            "is_super": p.is_super,
            "accessible_domains": empty_strings.clone(),
            "send_as": p.send_as,
        }),
        None => serde_json::json!({
            "permissions": empty_strings.clone(),
            "is_super": false,
            "accessible_domains": empty_strings.clone(),
            "send_as": empty_strings.clone(),
        }),
    };
    let blob = serde_json::json!({
        "address": acct.public.address,
        "display_name": acct.public.display_name,
        "permissions": perms_obj,
        "created_at_unix": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    });
    let blob_bytes = match serde_json::to_vec(&blob) {
        Ok(b) => b,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let kevy_url = std::env::var("MAILRS_KEVY_URL").ok();
    if let Some(url) = kevy_url {
        let token_clone = token.clone();
        let addr_clone = acct.public.address.clone();
        let _ = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            let mut c = kevy_client::Connection::open(&url)?;
            let key = format!("session:{token_clone}");
            c.set_with_ttl(
                key.as_bytes(),
                &blob_bytes,
                std::time::Duration::from_secs(7 * 24 * 3600),
            )?;
            // Per-user session index — makes it possible to revoke all
            // active sessions when the password changes.
            let idx = format!("session:by_addr:{addr_clone}");
            c.sadd(idx.as_bytes(), &[token_clone.as_bytes()])?;
            Ok(())
        })
        .await;
    } else {
        tracing::warn!("login: MAILRS_KEVY_URL unset — token NOT persisted");
    }

    let display = acct.public.display_name.clone();
    let address = acct.public.address.clone();
    crate::handlers::audit::record(&address, "auth.login", &address, "");
    let perms_vec = perms.map(|p| p.permissions).unwrap_or_default();
    let body = Json(LoginResponse {
        address,
        display_name: display,
        permissions: perms_vec,
        token: token.clone(),
    });
    let cookie =
        format!("mailrs_session={token}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=604800");

    let mut resp = body.into_response();
    if let Ok(v) = axum::http::HeaderValue::from_str(&cookie) {
        resp.headers_mut().insert(SET_COOKIE, v);
    }
    resp
}

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
        .fast()
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
    let acct = match state.fast().get_account_with_hash(&req.address).await {
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
    let totp_required =
        crate::handlers::kevy_util::with_kevy(move |c| c.hget(totp_key.as_bytes(), b"enabled"))
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
        .fast()
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
    let secret =
        match crate::handlers::kevy_util::with_kevy(move |c| c.hget(key_r.as_bytes(), b"secret")) {
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
    let enabled =
        crate::handlers::kevy_util::with_kevy(move |c| c.hget(enabled_key.as_bytes(), b"enabled"))
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
        // Recovery codes consumed one-shot.
        let rc_key = key.clone();
        let recovery_str = crate::handlers::kevy_util::with_kevy(move |c| {
            c.hget(rc_key.as_bytes(), b"recovery_codes")
        })
        .ok()
        .flatten()
        .and_then(|v| String::from_utf8(v).ok())
        .unwrap_or_default();
        let mut codes: Vec<&str> = recovery_str.split(',').filter(|s| !s.is_empty()).collect();
        let pos = codes.iter().position(|c| *c == req.code);
        if let Some(idx) = pos {
            codes.remove(idx);
            let joined = codes.join(",");
            let write_key = key.clone();
            let _ = crate::handlers::kevy_util::with_kevy(move |c| {
                c.hset(
                    write_key.as_bytes(),
                    &[(b"recovery_codes" as &[u8], joined.as_bytes())],
                )?;
                Ok(())
            });
            true
        } else {
            false
        }
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
    let acct = match state.fast().get_account_with_hash(&address).await {
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
    if let Err(e) = state.fast().set_account_password(&address, &req).await {
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

/// POST /api/auth/logout
///
/// Deletes the kevy `session:<token>` blob. The cookie is also cleared
/// via `Set-Cookie: mailrs_session=; Max-Age=0`.
pub async fn logout(req: axum::extract::Request) -> Response {
    let token = req
        .headers()
        .get(axum::http::header::COOKIE)
        .and_then(|h| h.to_str().ok())
        .and_then(|raw| {
            raw.split(';').find_map(|c| {
                let c = c.trim();
                c.strip_prefix("mailrs_session=").map(|s| s.to_string())
            })
        });

    if let (Some(t), Ok(url)) = (token, std::env::var("MAILRS_KEVY_URL")) {
        let _ = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            let mut c = kevy_client::Connection::open(&url)?;
            let key = format!("session:{t}");
            let _ = c.del(&[key.as_bytes()])?;
            Ok(())
        })
        .await;
    }

    let cookie = "mailrs_session=; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=0";
    let mut resp = StatusCode::NO_CONTENT.into_response();
    if let Ok(v) = axum::http::HeaderValue::from_str(cookie) {
        resp.headers_mut().insert(SET_COOKIE, v);
    }
    resp
}

/// GET /api/auth/me
pub async fn auth_me(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(address)): Extension<AuthedUser>,
    Extension(AuthedDisplayName(display_name)): Extension<AuthedDisplayName>,
) -> Result<Json<AuthMeResponse>, StatusCode> {
    let perms = state
        .fast()
        .effective_permissions(&address)
        .await
        .map_err(map_err)?;
    // `accessible_domains` lives in EffectivePermissions but is NOT in
    // the wire response (server cement only exposes is_super + send_as +
    // permissions on the EffectivePermissionsResponse). For Phase 3 the
    // accessible_domains field returns empty; the frontend ignores it for
    // non-admin users anyway. Full parity lands in checklist 3.20.
    Ok(Json(AuthMeResponse {
        address: perms.address.clone(),
        display_name,
        permissions: perms.permissions,
        accessible_domains: Vec::new(),
        send_as: perms.send_as,
    }))
}
