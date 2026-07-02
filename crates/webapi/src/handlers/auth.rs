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
        let _ = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            let mut c = kevy_client::Connection::open(&url)?;
            let key = format!("session:{token_clone}");
            c.set_with_ttl(
                key.as_bytes(),
                &blob_bytes,
                std::time::Duration::from_secs(7 * 24 * 3600),
            )?;
            Ok(())
        })
        .await;
    } else {
        tracing::warn!("login: MAILRS_KEVY_URL unset — token NOT persisted");
    }

    let display = acct.public.display_name.clone();
    let address = acct.public.address.clone();
    let perms_vec = perms.map(|p| p.permissions).unwrap_or_default();
    let body = Json(LoginResponse {
        address,
        display_name: display,
        permissions: perms_vec,
        token: token.clone(),
    });
    let cookie =
        format!("mailrs_session={token}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=3600");

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

    // Patch the account blob (mailrs:account:<addr> hash, blob field).
    let key = format!("mailrs:account:{address}");
    let key_read = key.clone();
    let cur = match crate::handlers::kevy_util::with_kevy(move |c| {
        c.hget(key_read.as_bytes(), b"blob")
    }) {
        Ok(Some(v)) => v,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(s) => return s.into_response(),
    };
    let mut val: serde_json::Value = match serde_json::from_slice(&cur) {
        Ok(v) => v,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if let Some(obj) = val.as_object_mut() {
        obj.insert(
            "password_hash".to_string(),
            serde_json::Value::String(new_hash),
        );
    }
    let payload = match serde_json::to_vec(&val) {
        Ok(v) => v,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if let Err(s) = crate::handlers::kevy_util::with_kevy(move |c| {
        c.hset(key.as_bytes(), &[(b"blob", payload.as_slice())])?;
        Ok(())
    }) {
        return s.into_response();
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({"success": true})),
    )
        .into_response()
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
