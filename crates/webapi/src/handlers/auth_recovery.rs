//! Password recovery, TOTP, and the encryption-key status the settings
//! page reads.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, State},
    http::StatusCode,
};

use crate::WebState;
use crate::handlers::complete::*;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

#[derive(Debug, serde::Deserialize)]
pub struct ForgotPasswordRequest {
    pub address: String,
    /// The recovery address the caller claims. Verified against the
    /// account; a mismatch is answered 204 with nothing sent, so the
    /// endpoint cannot be used to discover either the account or its
    /// recovery address.
    #[serde(default)]
    pub recovery_email: String,
}

/// POST /api/auth/forgot-password — verify the claimed recovery address,
/// issue a reset token with a 1-hour TTL, and mail the link to that
/// address.
///
/// Two things were missing until 2026-07-30, and together they made the
/// feature inert:
///
/// * **The recovery address was not checked.** The client sends it and the
///   monolith verifies it against the account
///   (`crates/server/src/web/auth/password.rs`); this handler did not name
///   the field, so it was dropped and a token was issued for any address
///   on request.
/// * **The token was never delivered.** It was written to kevy and left
///   there, so the UI said "check your inbox" and no mail ever arrived.
///
/// Every path answers 204. Whether the account exists, and whether the
/// recovery address matched, must not be observable — otherwise this
/// becomes an oracle for both.
pub async fn forgot_password(
    State(state): State<Arc<WebState>>,
    Json(req): Json<ForgotPasswordRequest>,
) -> Result<StatusCode, StatusCode> {
    // Rate-limit — one reset per address per 5 minutes. Without this
    // an attacker can spam the endpoint to churn pwreset:<token>
    // entries and DoS the reset flow.
    let rate_key = format!("pwreset:ratelimit:{}", req.address);
    let rate_key_c = rate_key.clone();
    let now = now_secs();
    let recent = with_kevy(move |c| c.get(rate_key_c.as_bytes()).map_err(std::io::Error::from))?
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    if now - recent < 300 {
        // Return 204 anyway so we don't leak "user exists" — the
        // client can't tell whether we actually issued a token.
        return Ok(StatusCode::NO_CONTENT);
    }
    // The claimed recovery address must match the one on the account.
    // Ported from the monolith, including the case-insensitive compare and
    // the "empty stored value never matches" rule — an account with no
    // recovery address cannot be reset this way.
    if req.recovery_email.trim().is_empty() {
        return Ok(StatusCode::NO_CONTENT);
    }
    let stored = account_recovery_email(&state, &req.address).await;
    let matches = stored
        .as_deref()
        .filter(|s| !s.is_empty())
        .is_some_and(|s| s.eq_ignore_ascii_case(req.recovery_email.trim()));
    if !matches {
        tracing::debug!(address = %req.address, "forgot-password: recovery address mismatch");
        return Ok(StatusCode::NO_CONTENT);
    }
    let recovery_email = req.recovery_email.trim().to_string();

    let token = random_hex(24);
    // The kevy closure takes ownership of `token`; the link needs it after.
    let token_for_link = token.clone();
    let key = format!("pwreset:{token}");
    let addr = req.address;
    let addr_c = addr.clone();
    // Needed for the mail body after the closure consumes `addr`.
    let addr_for_mail = addr.clone();
    with_kevy(move |c| {
        c.hset(
            key.as_bytes(),
            &[
                (b"address" as &[u8], addr_c.as_bytes()),
                (b"issued_at", now.to_string().as_bytes()),
            ],
        )?;
        let _ = c.expire(
            format!("pwreset:{token}").as_bytes(),
            std::time::Duration::from_secs(3600),
        );
        c.set(
            format!("pwreset_by_addr:{addr}").as_bytes(),
            token.as_bytes(),
        )?;
        // Bump rate-limit stamp with a matching TTL so the entry
        // self-clears after 5 minutes without cluttering kevy.
        c.set_with_ttl(
            rate_key.as_bytes(),
            now.to_string().as_bytes(),
            std::time::Duration::from_secs(300),
        )?;
        Ok(())
    })?;

    // The link has to point at this deployment. `MAILRS_HOSTNAME` is what
    // prod sets and what the monolith hardcoded to the same value.
    let hostname = std::env::var("MAILRS_HOSTNAME").unwrap_or_default();
    let reset_link = format!("https://{hostname}/reset-password?token={token_for_link}");
    let addr = addr_for_mail;
    let text_body = format!(
        "You requested a password reset for {addr}.\n\n\
         Open the link below to choose a new password:\n\
         {reset_link}\n\n\
         The link expires in 1 hour.\n\n\
         If you did not request this, you can ignore this email."
    );
    let html_body = format!(
        "<p>You requested a password reset for <strong>{addr}</strong>.</p>\
         <p><a href=\"{reset_link}\">Choose a new password</a></p>\
         <p>The link expires in 1 hour.</p>\
         <p>If you did not request this, you can ignore this email.</p>"
    );
    // A send failure is logged, not surfaced: the response must look the
    // same whether or not mail went out.
    if let Err(status) = super::prefs::send_system_mail(
        &recovery_email,
        "Password reset request",
        &text_body,
        &html_body,
    ) {
        tracing::error!(?status, address = %addr, "forgot-password: reset mail not enqueued");
    }
    Ok(StatusCode::NO_CONTENT)
}

/// The recovery address stored on an account, or `None`.
///
/// Read out of the account JSON rather than a typed field, the same way
/// `get_recovery_email` does — it is flattened into `AccountWithHashWire`
/// and reading it by name survives wire changes.
pub(crate) async fn account_recovery_email(state: &Arc<WebState>, address: &str) -> Option<String> {
    let acct = state.core.get_account_with_hash(address).await.ok()?;
    let raw = serde_json::to_value(&acct).ok()?;
    raw.get("recovery_email")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

#[derive(Debug, serde::Deserialize)]
pub struct ResetPasswordRequest {
    pub token: String,
    pub new_password: String,
}

/// POST /api/auth/reset-password — verify the token, delegate the
/// hash write to fastcore, then invalidate the token.
pub async fn reset_password(
    State(state): State<Arc<WebState>>,
    Json(req): Json<ResetPasswordRequest>,
) -> Result<StatusCode, StatusCode> {
    use argon2::{
        Argon2,
        password_hash::{PasswordHasher, SaltString, rand_core::OsRng as ArgonRng},
    };
    let token = req.token.clone();
    let addr_bytes = with_kevy(move |c| {
        c.hget(format!("pwreset:{token}").as_bytes(), b"address")
            .map_err(std::io::Error::from)
    })?;
    let Some(addr_bytes) = addr_bytes else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    let address = String::from_utf8_lossy(&addr_bytes).to_string();
    let salt = SaltString::generate(&mut ArgonRng);
    let hash = Argon2::default()
        .hash_password(req.new_password.as_bytes(), &salt)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .to_string();
    let set_req = mailrs_core_api::method::admin::SetPasswordRequest {
        password_hash: hash,
    };
    state
        .core
        .set_account_password(&address, &set_req)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    // Revoke all existing sessions for this address so an attacker who
    // captured a token via the victim's old device can't keep using
    // it after the victim resets. Mirrors auth.rs::change_password.
    let addr_c = address.clone();
    let _ = with_kevy(move |c| {
        let idx = format!("session:by_addr:{addr_c}");
        let tokens = c.smembers(idx.as_bytes()).unwrap_or_default();
        for t in tokens {
            let key = format!("session:{}", String::from_utf8_lossy(&t));
            let _ = c.del(&[key.as_bytes()]);
        }
        let _ = c.del(&[idx.as_bytes()]);
        Ok(())
    });
    let tok = req.token;
    with_kevy(move |c| {
        c.del(&[format!("pwreset:{tok}").as_bytes()])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/auth/recovery-email — returns the account's recovery
/// email (or null). POST updates it.
///
/// Reads via the fastcore RPC because the blob lives in fastcore's
/// embedded kevy (`upsert_account`). Prior version read
/// `mailrs:account:<u>` from the network kevy where it was never
/// written — so the endpoint always returned null even after a
/// successful save.
pub async fn get_recovery_email(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let acct = match state.core.get_account_with_hash(&user).await {
        Ok(a) => a,
        Err(_) => {
            return Ok(Json(serde_json::json!({ "recovery_email": null })));
        }
    };
    // Recovery email lives on the AccountWithHashWire's inner
    // AccountWire, which is flattened into the same JSON blob. Read
    // it back via serde_json to future-proof against wire changes.
    let raw = match serde_json::to_value(&acct) {
        Ok(v) => v,
        Err(_) => return Ok(Json(serde_json::json!({ "recovery_email": null }))),
    };
    let rec = raw
        .get("recovery_email")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    Ok(Json(serde_json::json!({ "recovery_email": rec })))
}

#[derive(Debug, serde::Deserialize)]
pub struct SetRecoveryEmailRequest {
    pub recovery_email: Option<String>,
}

pub async fn set_recovery_email(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<SetRecoveryEmailRequest>,
) -> Result<StatusCode, StatusCode> {
    let email = req.recovery_email.unwrap_or_default();
    let wire_req = mailrs_core_api::method::admin::UpdateRecoveryEmailRequest {
        recovery_email: email,
    };
    state
        .core
        .set_recovery_email(&user, &wire_req)
        .await
        .map_err(|e| {
            StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// TOTP enrollment storage layout (network kevy):
///
///   totp:<addr>            hash
///     secret               base32 secret
///     enabled              "0" | "1"
///     recovery_codes       CSV of 8-char hex codes
///
/// Mirrors the monolith schema at `domain_store.save_totp_secret` /
/// `get_totp_secret` / `enable_totp` / `disable_totp` — only the
/// backend differs.
#[derive(Debug, serde::Deserialize)]
pub struct TotpCodeRequest {
    pub code: String,
}

/// GET /api/auth/totp/status — returns `{ enabled: bool, address }`.
pub async fn totp_status(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Json<serde_json::Value> {
    let key = format!("totp:{user}");
    let key_c = key.clone();
    let enabled = with_kevy(move |c| {
        c.hget(key_c.as_bytes(), b"enabled")
            .map_err(std::io::Error::from)
    })
    .ok()
    .flatten()
    .map(|v| v == b"1")
    .unwrap_or(false);
    Json(serde_json::json!({
        "enabled": enabled,
        "address": user,
    }))
}

/// POST /api/auth/totp/setup — generate a new secret + 8 recovery
/// codes, store them un-enabled, return the secret / otpauth URL /
/// recovery codes so the client can render the QR.
pub async fn totp_setup(
    Extension(AuthedUser(address)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let secret = crate::handlers::totp_util::generate_secret();
    let recovery_codes = crate::handlers::totp_util::generate_recovery_codes();
    let recovery_str = recovery_codes.join(",");
    let otpauth_url = crate::handlers::totp_util::get_otpauth_url(&secret, &address, "mailrs");

    let key = format!("totp:{address}");
    let s = secret.clone();
    let r = recovery_str.clone();
    with_kevy(move |c| {
        c.hset(
            key.as_bytes(),
            &[
                (b"secret" as &[u8], s.as_bytes()),
                (b"enabled", b"0"),
                (b"recovery_codes", r.as_bytes()),
            ],
        )?;
        Ok(())
    })?;
    Ok(Json(serde_json::json!({
        "secret": secret,
        "otpauth_url": otpauth_url,
        "recovery_codes": recovery_codes,
    })))
}

pub async fn totp_enable(
    Extension(AuthedUser(address)): Extension<AuthedUser>,
    Json(req): Json<TotpCodeRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = format!("totp:{address}");
    let key_r = key.clone();
    let secret = with_kevy(move |c| {
        c.hget(key_r.as_bytes(), b"secret")
            .map_err(std::io::Error::from)
    })?
    .and_then(|v| String::from_utf8(v).ok())
    .ok_or(StatusCode::BAD_REQUEST)?;
    let enabled = with_kevy({
        let k = key.clone();
        move |c| {
            c.hget(k.as_bytes(), b"enabled")
                .map_err(std::io::Error::from)
        }
    })?
    .map(|v| v == b"1")
    .unwrap_or(false);
    if enabled {
        return Err(StatusCode::BAD_REQUEST);
    }
    if !crate::handlers::totp_util::verify_code(&secret, &req.code) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    with_kevy(move |c| {
        c.hset(key.as_bytes(), &[(b"enabled" as &[u8], b"1")])?;
        Ok(())
    })?;
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn totp_disable(
    Extension(AuthedUser(address)): Extension<AuthedUser>,
    Json(req): Json<TotpCodeRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = format!("totp:{address}");
    let key_r = key.clone();
    let secret = with_kevy(move |c| {
        c.hget(key_r.as_bytes(), b"secret")
            .map_err(std::io::Error::from)
    })?
    .and_then(|v| String::from_utf8(v).ok())
    .ok_or(StatusCode::BAD_REQUEST)?;
    let enabled_key = key.clone();
    let enabled = with_kevy(move |c| {
        c.hget(enabled_key.as_bytes(), b"enabled")
            .map_err(std::io::Error::from)
    })?
    .map(|v| v == b"1")
    .unwrap_or(false);
    if !enabled {
        return Err(StatusCode::BAD_REQUEST);
    }
    if !crate::handlers::totp_util::verify_code(&secret, &req.code) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    with_kevy(move |c| {
        c.del(&[key.as_bytes()])?;
        Ok(())
    })?;
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn keys_status(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = format!("pgp_keys:{user}");
    let flat = with_kevy(move |c| c.hgetall(key.as_bytes()).map_err(std::io::Error::from))?;
    let count = flat.len() / 2;
    Ok(Json(serde_json::json!({
        "configured": count > 0,
        "key_count": count,
    })))
}
