//! Fastcore-native handlers for every remaining route the React UI can
//! hit. Missing routes were making the dashboard / admin / password
//! reset flows either 404 or 500. Fill them all in — real
//! implementations where possible, safe empty defaults where the
//! feature isn't wired up yet.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

fn with_kevy<F, T>(f: F) -> Result<T, StatusCode>
where
    F: FnOnce(&mut kevy_client::Connection) -> std::io::Result<T> + Send + 'static,
    T: Send + 'static,
{
    let url = std::env::var("MAILRS_KEVY_URL").map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let handle = std::thread::spawn(move || -> std::io::Result<T> {
        let mut c = kevy_client::Connection::connect(&url).map_err(std::io::Error::other)?;
        f(&mut c)
    });
    handle
        .join()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn hgetall_values(c: &mut kevy_client::Connection, key: &str) -> std::io::Result<Vec<Vec<u8>>> {
    let flat = c.hgetall(key.as_bytes()).map_err(std::io::Error::other)?;
    Ok(flat
        .into_iter()
        .enumerate()
        .filter_map(|(i, v)| if i % 2 == 1 { Some(v) } else { None })
        .collect())
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn next_id(c: &mut kevy_client::Connection, counter_key: &str) -> std::io::Result<i64> {
    // v2 Stage B.2: single-op INCR — kevy-side atomic, no read-modify-
    // write race. Prior get + parse + set could let two concurrent
    // /api/prefs writes both read the same current value and both
    // set the same next id, losing one row.
    c.incr(counter_key.as_bytes())
        .map_err(std::io::Error::other)
}

fn random_hex(bytes: usize) -> String {
    let mut b = vec![0u8; bytes];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn map_core_err(e: mailrs_core_api::error::CoreApiError) -> StatusCode {
    StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

// ── /api/mail/stats — dashboard summary ────────────────────────────

/// GET /api/mail/stats — combined dashboard counters from fastcore.
/// Runs three fastcore RPCs in parallel-like sequence and folds into
/// the shape `web/src/pages/dashboard.tsx` expects:
/// `{ categories, storage_bytes, total_messages, unread_messages }`.
pub async fn get_mail_stats(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let cats = state
        .core
        .conversation_categories(&user)
        .await
        .map_err(map_core_err)?;
    let unseen = state.core.unseen_count(&user).await.map_err(map_core_err)?;
    let total: i64 = cats.categories.iter().map(|c| c.count).sum();
    Ok(Json(serde_json::json!({
        "categories": cats.categories,
        "storage_bytes": 0,
        "total_messages": total,
        "unread_messages": unseen.count,
    })))
}

// ── Auth extras (OIDC / password reset / recovery / TOTP) ─────────

/// GET /api/auth/oidc/config — OIDC providers list. Empty → login
/// page hides the "Sign in with X" buttons cleanly.
///
/// v2.7.1 §Phase 12 §12.4 (2026-07-13): the pre-fix handler
/// returned `{enabled: false, providers: []}` unconditionally, so
/// the frontend login page never showed the OIDC button on prod
/// even when `MAILRS_OIDC_CLIENT_ID` / `MAILRS_OIDC_CLIENT_SECRET`
/// / `MAILRS_OIDC_ISSUER` were all set. Now mirrors the monolith
/// `web/auth/oidc.rs::oidc_client_config` gating: `enabled` is true
/// iff all three env vars are set, and one provider entry is
/// emitted with `id`, `name` (from `MAILRS_OIDC_PROVIDER_NAME` or
/// `"OIDC"`), and `login_url = /api/auth/oidc/login`.
pub async fn oidc_config() -> Json<serde_json::Value> {
    let enabled = std::env::var("MAILRS_OIDC_CLIENT_ID").is_ok()
        && std::env::var("MAILRS_OIDC_CLIENT_SECRET").is_ok()
        && std::env::var("MAILRS_OIDC_ISSUER").is_ok();
    let providers = if enabled {
        vec![serde_json::json!({
            "id": "primary",
            "name": std::env::var("MAILRS_OIDC_PROVIDER_NAME").unwrap_or_else(|_| "OIDC".into()),
            "login_url": "/api/auth/oidc/login",
        })]
    } else {
        Vec::new()
    };
    Json(serde_json::json!({
        "enabled": enabled,
        "providers": providers,
    }))
}

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
    let recent = with_kevy(move |c| c.get(rate_key_c.as_bytes()).map_err(std::io::Error::other))?
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
        )
        .map_err(std::io::Error::other)?;
        let _ = c.expire(
            format!("pwreset:{token}").as_bytes(),
            std::time::Duration::from_secs(3600),
        );
        c.set(
            format!("pwreset_by_addr:{addr}").as_bytes(),
            token.as_bytes(),
        )
        .map_err(std::io::Error::other)?;
        // Bump rate-limit stamp with a matching TTL so the entry
        // self-clears after 5 minutes without cluttering kevy.
        c.set_with_ttl(
            rate_key.as_bytes(),
            now.to_string().as_bytes(),
            std::time::Duration::from_secs(300),
        )
        .map_err(std::io::Error::other)?;
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
async fn account_recovery_email(state: &Arc<WebState>, address: &str) -> Option<String> {
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
            .map_err(std::io::Error::other)
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
        c.del(&[format!("pwreset:{tok}").as_bytes()])
            .map_err(std::io::Error::other)?;
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
            .map_err(std::io::Error::other)
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
        )
        .map_err(std::io::Error::other)?;
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
            .map_err(std::io::Error::other)
    })?
    .and_then(|v| String::from_utf8(v).ok())
    .ok_or(StatusCode::BAD_REQUEST)?;
    let enabled = with_kevy({
        let k = key.clone();
        move |c| {
            c.hget(k.as_bytes(), b"enabled")
                .map_err(std::io::Error::other)
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
        c.hset(key.as_bytes(), &[(b"enabled" as &[u8], b"1")])
            .map_err(std::io::Error::other)?;
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
            .map_err(std::io::Error::other)
    })?
    .and_then(|v| String::from_utf8(v).ok())
    .ok_or(StatusCode::BAD_REQUEST)?;
    let enabled_key = key.clone();
    let enabled = with_kevy(move |c| {
        c.hget(enabled_key.as_bytes(), b"enabled")
            .map_err(std::io::Error::other)
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
        c.del(&[key.as_bytes()]).map_err(std::io::Error::other)?;
        Ok(())
    })?;
    Ok(Json(serde_json::json!({ "success": true })))
}

// ── /api/mail/keys/status — PGP setup status ──────────────────────

pub async fn keys_status(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = format!("pgp_keys:{user}");
    let flat = with_kevy(move |c| c.hgetall(key.as_bytes()).map_err(std::io::Error::other))?;
    let count = flat.len() / 2;
    Ok(Json(serde_json::json!({
        "configured": count > 0,
        "key_count": count,
    })))
}

// ── /api/mail/messages/{uid} — single message (metadata + body) ───

pub async fn get_message_single(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(uid): Path<u32>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let w = state
        .core
        .get_message_by_uid_for_user(&user, uid)
        .await
        .map_err(map_core_err)?;
    let maildir_root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let mut text_body: Option<String> = None;
    let mut html_body: Option<String> = None;
    if let Some((local, domain)) = user.split_once('@') {
        let path = format!("{maildir_root}/{domain}/{local}");
        use mailrs_message_store::MessageStore;
        let store = mailrs_message_store::MaildirStore;
        let id = mailrs_message_store::MessageId(w.blob_ref.clone());
        if let Ok(Some(bytes)) = store.fetch(&path, &id).await {
            let root = mailrs_mime::parse(&bytes);
            for part in root.walk() {
                let mt = part.content_type.mime_type();
                if text_body.is_none() && mt == "text/plain" {
                    text_body = part.body_text();
                } else if html_body.is_none() && mt == "text/html" {
                    html_body = part.body_text();
                }
                if text_body.is_some() && html_body.is_some() {
                    break;
                }
            }
        }
    }
    // NOTE: legacy `id` field dropped 2026-07-08 (was always 0 under
    // fastcore's kevy-only architecture). Callers must use `uid` as
    // the per-user unique identity. See ThreadMessageResponse for the
    // matching rationale.
    Ok(Json(serde_json::json!({
        "uid": w.uid,
        "sender": mailrs_rfc2047::decode(w.sender.as_bytes()).into_owned(),
        "recipients": mailrs_rfc2047::decode(w.recipients.as_bytes()).into_owned(),
        "subject": w.subject,
        "internal_date": w.internal_date,
        "message_id": w.message_id,
        "text_body": text_body,
        "html_body": html_body,
        "flags": w.flags,
    })))
}

// ── /api/queue/{id}/retry — outbound queue retry ──────────────────

pub async fn queue_retry(Path(id): Path<i64>) -> Result<StatusCode, StatusCode> {
    // Set state=pending on the v2 job hash and LPUSH pending-idx —
    // the same shape sender's own retry path uses. The pre-2.9.38 form
    // wrote the legacy `mailrs:outbound:pending`, which sender had
    // stopped consuming, so retries silently no-op'd.
    let now = now_secs();
    with_kevy(move |c| {
        mailrs_core_sidestate::families::outbound::requeue_pending(c, id, now).map(|_| ())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Calendar (CalDAV placeholder) ──────────────────────────────────

pub async fn calendar_feeds() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "items": [] }))
}

#[derive(Debug, serde::Deserialize)]
pub struct CalendarConflictsQuery {
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub end: Option<String>,
}

pub async fn calendar_conflicts(
    Query(_q): Query<CalendarConflictsQuery>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "conflicts": [] }))
}

// ── Admin: apps ────────────────────────────────────────────────────

const APPS_KEY: &str = "admin:apps";
const APPS_CTR: &str = "admin:apps:counter";

pub async fn list_apps() -> Result<Json<serde_json::Value>, StatusCode> {
    let vals = with_kevy(|c| hgetall_values(c, APPS_KEY))?;
    let items: Vec<serde_json::Value> = vals
        .into_iter()
        .filter_map(|v| serde_json::from_slice(&v).ok())
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

#[derive(Debug, serde::Deserialize)]
pub struct CreateAppRequest {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub scopes: Vec<String>,
}

pub async fn create_app(
    Json(req): Json<CreateAppRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use sha2::{Digest, Sha256};
    let id = with_kevy(|c| next_id(c, APPS_CTR))?;
    let app_id = format!("app_{id}");
    let secret = random_hex(32);
    // Store the sha256 of the secret so /oauth/token can verify
    // what an app presents without persisting the plaintext (matches
    // how the monolith stored api_keys).
    let secret_sha = format!("{:x}", Sha256::digest(secret.as_bytes()));
    let blob = serde_json::json!({
        "id": id,
        "app_id": app_id,
        "name": req.name,
        "scopes": req.scopes,
        "created_at": now_secs(),
        "secret_sha256": secret_sha,
    });
    let payload = serde_json::to_vec(&blob).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    with_kevy(move |c| {
        c.hset(
            APPS_KEY.as_bytes(),
            &[(id.to_string().as_bytes(), payload.as_slice())],
        )
        .map_err(std::io::Error::other)?;
        Ok(())
    })?;
    // Secret is returned once — the caller is responsible for storing
    // it; subsequent GETs only see the sha256.
    Ok(Json(serde_json::json!({
        "id": id,
        "app_id": app_id,
        "secret": secret,
    })))
}

pub async fn get_app(Path(app_id): Path<String>) -> Result<Json<serde_json::Value>, StatusCode> {
    let vals = with_kevy(|c| hgetall_values(c, APPS_KEY))?;
    for v in vals {
        if let Ok(app) = serde_json::from_slice::<serde_json::Value>(&v)
            && app.get("app_id").and_then(|v| v.as_str()) == Some(app_id.as_str())
        {
            return Ok(Json(app));
        }
    }
    Err(StatusCode::NOT_FOUND)
}

pub async fn delete_app(Path(app_id): Path<String>) -> Result<StatusCode, StatusCode> {
    let vals = with_kevy(|c| hgetall_values(c, APPS_KEY))?;
    for v in vals {
        if let Ok(app) = serde_json::from_slice::<serde_json::Value>(&v)
            && app.get("app_id").and_then(|v| v.as_str()) == Some(app_id.as_str())
            && let Some(id) = app.get("id").and_then(|v| v.as_i64())
        {
            with_kevy(move |c| {
                c.hdel(APPS_KEY.as_bytes(), &[id.to_string().as_bytes()])
                    .map_err(std::io::Error::other)?;
                Ok(())
            })?;
            return Ok(StatusCode::NO_CONTENT);
        }
    }
    Err(StatusCode::NOT_FOUND)
}

// ── Admin: audit-log messages/raw + audit/accounts + audit/conversations ────

/// GET /api/admin/audit/accounts — return the registered accounts
/// shaped for the audit panel.
///
/// v2.2-fix (2026-07-09): pre-fix version read from network-kevy set
/// `mailrs:accounts:index` — a legacy key that fastcore never wrote
/// to after the split; the set was empty in prod so the audit page
/// showed "No auditable accounts" even for super-admin. Now calls
/// fastcore's `list_accounts` RPC (embedded kevy — where the
/// accounts actually live). Populates address / domain / display_name
/// / active shape the frontend `AuditAccount` type expects.
pub async fn audit_accounts(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let accounts = state
        .core
        .list_accounts()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let items: Vec<serde_json::Value> = accounts
        .items
        .into_iter()
        .map(|a| {
            serde_json::json!({
                "address": a.address,
                "domain": a.domain,
                "display_name": a.display_name,
                "active": a.active,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

#[derive(Debug, serde::Deserialize)]
pub struct AuditConversationsQuery {
    pub user: Option<String>,
    #[serde(default = "default_audit_conv_limit")]
    pub limit: u32,
}

fn default_audit_conv_limit() -> u32 {
    100
}

/// GET /api/admin/audit/conversations?user=&limit= — list threads
/// for the target user via fastcore RPC. Same shape as normal
/// `/api/conversations` but scoped to any user (admin impersonation).
pub async fn audit_conversations(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    axum::extract::Query(q): axum::extract::Query<AuditConversationsQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let Some(target) = q.user else {
        return Ok(Json(serde_json::json!({ "items": [] })));
    };
    let req = mailrs_core_api::method::conversation::ListConversationsRequest {
        filter: mailrs_core_api::types::ConversationFilter {
            limit: q.limit,
            ..Default::default()
        },
    };
    let resp = state
        .core
        .list_conversations(&target, &req)
        .await
        .map_err(|e| {
            StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
        })?;
    let items: Vec<serde_json::Value> = resp
        .items
        .into_iter()
        .map(|c| {
            serde_json::json!({
                "thread_id": c.thread_id,
                "subject": c.subject,
                "participants": c.participants,
                "message_count": c.message_count,
                "last_date": c.last_date,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({
        "user": target,
        "items": items,
    })))
}

/// GET /api/admin/audit/conversations/{thread_id} — thread summary
/// for admin audit. Returns thread aggregate fields (subject,
/// participants, count) but NOT the message list — use
/// `.../messages` for that.
pub async fn audit_conversation_detail(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Best we can do without a user context: read thread aggregate
    // directly from network kevy. Fastcore's per-user RPCs need a user.
    let key = format!("mailrs:thread:{thread_id}");
    let flat = with_kevy(move |c| c.hgetall(key.as_bytes()).map_err(std::io::Error::other))?;
    if flat.is_empty() {
        return Ok(Json(
            serde_json::json!({ "thread_id": thread_id, "found": false }),
        ));
    }
    let mut obj = serde_json::Map::new();
    let mut i = 0;
    while i + 1 < flat.len() {
        let k = String::from_utf8_lossy(&flat[i]).to_string();
        let v = String::from_utf8_lossy(&flat[i + 1]).to_string();
        obj.insert(k, serde_json::Value::String(v));
        i += 2;
    }
    obj.insert(
        "thread_id".into(),
        serde_json::Value::String(thread_id.clone()),
    );
    obj.insert("found".into(), serde_json::Value::Bool(true));
    Ok(Json(serde_json::Value::Object(obj)))
}

#[derive(Debug, serde::Deserialize)]
pub struct AuditConvMessagesQuery {
    pub user: String,
}

/// GET /api/admin/audit/conversations/{thread_id}/messages?user=
/// — the message list for a thread scoped to a target user.
pub async fn audit_conversation_messages(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
    axum::extract::Query(q): axum::extract::Query<AuditConvMessagesQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let resp = state
        .core
        .list_thread_messages(&q.user, &thread_id)
        .await
        .map_err(|e| {
            StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
        })?;
    Ok(Json(serde_json::json!({
        "thread_id": thread_id,
        "user": q.user,
        "items": resp.items,
    })))
}

/// GET /api/admin/audit/messages/{uid}/raw?user= — fetch raw envelope
/// bytes for a message under an impersonated user. Reads maildir.
pub async fn audit_message_raw(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(uid): Path<u32>,
    axum::extract::Query(q): axum::extract::Query<AuditConvMessagesQuery>,
) -> Result<axum::response::Response, StatusCode> {
    let wire = state
        .core
        .get_message_by_uid_for_user(&q.user, uid)
        .await
        .map_err(|e| StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::NOT_FOUND))?;
    let maildir_root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let Some((local, domain)) = q.user.split_once('@') else {
        return Err(StatusCode::BAD_REQUEST);
    };
    let path = format!("{maildir_root}/{domain}/{local}");
    let store = mailrs_message_store::MaildirStore;
    use mailrs_message_store::MessageStore;
    let id = mailrs_message_store::MessageId(wire.blob_ref);
    match store.fetch(&path, &id).await {
        Ok(Some(bytes)) => axum::response::Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "message/rfc822")
            .body(axum::body::Body::from(bytes))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR),
        _ => Err(StatusCode::NOT_FOUND),
    }
}

// ── Admin: config/smtp + system-config ─────────────────────────────

pub async fn get_smtp_config() -> Result<Json<serde_json::Value>, StatusCode> {
    // Prefer an operator-provided override in kevy (set via
    // `set_smtp_config`), otherwise synthesise the shape the admin UI
    // expects from the process env. The webapi doesn't own the SMTP
    // listeners in the fastcore split — `mailrs-receiver` does — so
    // the ports come from the same env vars the receiver reads.
    let key = b"admin:config:smtp".to_vec();
    if let Ok(Some(bytes)) = with_kevy(move |c| c.get(&key).map_err(std::io::Error::other))
        && let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes)
    {
        return Ok(Json(v));
    }
    fn env_u16(name: &str, default: u16) -> u16 {
        std::env::var(name)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(default)
    }
    let hostname = std::env::var("MAILRS_HOSTNAME").unwrap_or_else(|_| "mail.example.com".into());
    let domains: Vec<String> = std::env::var("MAILRS_LOCAL_DOMAINS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let tls_enabled = std::env::var("MAILRS_TLS_ENABLED")
        .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
        .unwrap_or(true);
    let max_message_size = std::env::var("MAILRS_MAX_MESSAGE_SIZE_BYTES")
        .ok()
        .and_then(|s| s.parse::<u64>().ok());
    let mut out = serde_json::json!({
        "hostname": hostname,
        "smtp_port": env_u16("MAILRS_SMTP_PORT", 25),
        "submission_port": env_u16("MAILRS_SUBMISSION_PORT", 587),
        "imap_port": env_u16("MAILRS_IMAP_PORT", 143),
        "local_domains": domains,
        "tls_enabled": tls_enabled,
    });
    if let Some(sz) = max_message_size
        && let Some(o) = out.as_object_mut()
    {
        o.insert("max_message_size".into(), serde_json::json!(sz));
    }
    Ok(Json(out))
}

pub async fn set_smtp_config(Json(cfg): Json<serde_json::Value>) -> Result<StatusCode, StatusCode> {
    let payload = serde_json::to_vec(&cfg).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    with_kevy(move |c| {
        c.set(b"admin:config:smtp", payload.as_slice())
            .map_err(std::io::Error::other)?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/admin/system-config
///
/// Returns the `{success, entries}` envelope the admin UI expects.
/// Each entry describes a single tunable — its current value, where
/// the value came from (env / database / default), and enough metadata
/// for the UI to render an editor.
///
/// The fastcore lane treats runtime tuning as a small collection of
/// well-known keys rather than a fully-dynamic catalog. If the operator
/// has overridden a key via `POST /api/admin/system-config/{k}`, it's
/// read from kevy; otherwise the `source: "env"` reading (or the built-
/// in default) wins. UI renders "Environment" pill next to the value.
pub async fn get_system_config() -> Result<Json<serde_json::Value>, StatusCode> {
    let flat = with_kevy(|c| {
        c.hgetall(b"admin:system-config")
            .map_err(std::io::Error::other)
    })?;
    let mut overrides: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut i = 0;
    while i + 1 < flat.len() {
        let k = String::from_utf8_lossy(&flat[i]).to_string();
        let v = String::from_utf8_lossy(&flat[i + 1]).to_string();
        overrides.insert(k, v);
        i += 2;
    }
    // The catalog is the union of what the operator has already
    // overridden and a small built-in list of tunables the UI wants to
    // surface even on a fresh install. Keeps the page useful when
    // kevy has no override rows yet.
    const CATALOG: &[(&str, &str, &str, &str, &str)] = &[
        // (key, group, description, env_var, default)
        (
            "hostname",
            "smtp",
            "Public SMTP hostname (HELO / greeting)",
            "MAILRS_HOSTNAME",
            "",
        ),
        (
            "smtp_port",
            "smtp",
            "Inbound SMTP port on the receiver process",
            "MAILRS_SMTP_PORT",
            "25",
        ),
        (
            "submission_port",
            "smtp",
            "Authenticated submission port",
            "MAILRS_SUBMISSION_PORT",
            "587",
        ),
        (
            "imap_port",
            "imap",
            "IMAP port on the fastcore process",
            "MAILRS_IMAP_PORT",
            "143",
        ),
        (
            "local_domains",
            "smtp",
            "Comma-separated list of accepted local domains",
            "MAILRS_LOCAL_DOMAINS",
            "",
        ),
        (
            "tls_enabled",
            "security",
            "Serve STARTTLS / IMAPS with certificates",
            "MAILRS_TLS_ENABLED",
            "true",
        ),
        (
            "max_message_size_bytes",
            "smtp",
            "Reject inbound mail larger than this (bytes)",
            "MAILRS_MAX_MESSAGE_SIZE_BYTES",
            "",
        ),
    ];
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut entries: Vec<serde_json::Value> = Vec::new();
    for (key, group, description, env_var, default) in CATALOG {
        seen.insert(key.to_string());
        let (value, source) = if let Some(v) = overrides.get(*key) {
            (v.clone(), "database")
        } else if let Ok(v) = std::env::var(env_var) {
            (v, "env")
        } else {
            (default.to_string(), "default")
        };
        entries.push(serde_json::json!({
            "key": key,
            "value": value,
            "default_value": default,
            "description": description,
            "group": group,
            "source": source,
            "value_type": "string",
        }));
    }
    // Any operator override that isn't in the built-in catalog still
    // gets surfaced so the UI can show / edit / remove it.
    for (k, v) in &overrides {
        if seen.contains(k) {
            continue;
        }
        entries.push(serde_json::json!({
            "key": k,
            "value": v,
            "default_value": "",
            "description": "Operator-defined key (no built-in metadata).",
            "group": "custom",
            "source": "database",
            "value_type": "string",
        }));
    }
    Ok(Json(serde_json::json!({
        "success": true,
        "entries": entries,
    })))
}

pub async fn set_system_config_key(
    Path(k): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<StatusCode, StatusCode> {
    let v = body
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| body.to_string());
    with_kevy(move |c| {
        c.hset(b"admin:system-config", &[(k.as_bytes(), v.as_bytes())])
            .map_err(std::io::Error::other)?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Admin: groups + permissions + group members ────────────────────

const GROUPS_KEY: &str = "admin:groups";
const GROUPS_CTR: &str = "admin:groups:counter";

pub async fn list_groups() -> Result<Json<serde_json::Value>, StatusCode> {
    let vals = with_kevy(|c| hgetall_values(c, GROUPS_KEY))?;
    let items: Vec<serde_json::Value> = vals
        .into_iter()
        .filter_map(|v| serde_json::from_slice(&v).ok())
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

#[derive(Debug, serde::Deserialize)]
pub struct CreateGroupRequest {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
}

pub async fn create_group(
    Json(req): Json<CreateGroupRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let id = with_kevy(|c| next_id(c, GROUPS_CTR))?;
    let g = serde_json::json!({
        "id": id,
        "name": req.name,
        "description": req.description,
        "created_at": now_secs(),
    });
    let payload = serde_json::to_vec(&g).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    with_kevy(move |c| {
        c.hset(
            GROUPS_KEY.as_bytes(),
            &[(id.to_string().as_bytes(), payload.as_slice())],
        )
        .map_err(std::io::Error::other)?;
        Ok(())
    })?;
    Ok(Json(g))
}

pub async fn delete_group(Path(id): Path<i64>) -> Result<StatusCode, StatusCode> {
    with_kevy(move |c| {
        c.hdel(GROUPS_KEY.as_bytes(), &[id.to_string().as_bytes()])
            .map_err(std::io::Error::other)?;
        c.del(&[
            format!("admin:groups:{id}:permissions").as_bytes(),
            format!("admin:groups:{id}:members").as_bytes(),
        ])
        .map_err(std::io::Error::other)?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_group_permissions(
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = format!("admin:groups:{id}:permissions");
    let raw = with_kevy(move |c| c.smembers(key.as_bytes()).map_err(std::io::Error::other))?;
    let perms: Vec<String> = raw
        .into_iter()
        .filter_map(|b| String::from_utf8(b).ok())
        .collect();
    Ok(Json(serde_json::json!({ "permissions": perms })))
}

#[derive(Debug, serde::Deserialize)]
pub struct SetGroupPermissionsRequest {
    pub permissions: Vec<String>,
}

pub async fn set_group_permissions(
    Path(id): Path<i64>,
    Json(req): Json<SetGroupPermissionsRequest>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("admin:groups:{id}:permissions");
    with_kevy(move |c| {
        c.del(&[key.as_bytes()]).map_err(std::io::Error::other)?;
        let refs: Vec<&[u8]> = req.permissions.iter().map(|s| s.as_bytes()).collect();
        if !refs.is_empty() {
            c.sadd(key.as_bytes(), &refs)
                .map_err(std::io::Error::other)?;
        }
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_group_members(
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = format!("admin:groups:{id}:members");
    let raw = with_kevy(move |c| c.smembers(key.as_bytes()).map_err(std::io::Error::other))?;
    let members: Vec<String> = raw
        .into_iter()
        .filter_map(|b| String::from_utf8(b).ok())
        .collect();
    Ok(Json(serde_json::json!({ "members": members })))
}

#[derive(Debug, serde::Deserialize)]
pub struct AddGroupMemberRequest {
    pub address: String,
}

pub async fn add_group_member(
    Path(id): Path<i64>,
    Json(req): Json<AddGroupMemberRequest>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("admin:groups:{id}:members");
    let addr = req.address;
    with_kevy(move |c| {
        c.sadd(key.as_bytes(), &[addr.as_bytes()])
            .map_err(std::io::Error::other)?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_group_member(
    Path((id, address)): Path<(i64, String)>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("admin:groups:{id}:members");
    with_kevy(move |c| {
        c.srem(key.as_bytes(), &[address.as_bytes()])
            .map_err(std::io::Error::other)?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_permissions() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "permissions": [
            "mail.send", "mail.read", "mail.read_domain",
            "admin.domains", "admin.accounts", "admin.aliases",
            "admin.groups", "admin.queue", "admin.sieve",
            "admin.impersonate", "internal.rpc",
        ],
    }))
}

// ── Admin: email-groups (distribution lists) ──────────────────────

const EG_KEY: &str = "admin:email-groups";
const EG_CTR: &str = "admin:email-groups:counter";

pub async fn list_email_groups() -> Result<Json<serde_json::Value>, StatusCode> {
    let vals = with_kevy(|c| hgetall_values(c, EG_KEY))?;
    let items: Vec<serde_json::Value> = vals
        .into_iter()
        .filter_map(|v| serde_json::from_slice(&v).ok())
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

#[derive(Debug, serde::Deserialize)]
pub struct CreateEmailGroupRequest {
    #[serde(default)]
    pub address: String,
    /// The domain the group belongs to. Sent by the UI and stored by the
    /// monolith (`crates/server/src/web/admin/email_groups.rs`); this
    /// handler did not name it, so — every field here being defaulted, no
    /// 422 was raised — the value was dropped and the group was created
    /// without it.
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub name: String,
    /// Same as `domain`: sent, previously dropped.
    #[serde(default)]
    pub description: String,
    /// Initial membership. The UI creates groups empty and adds members
    /// through `POST /admin/email-groups/{id}/members`, so this stays
    /// defaulted rather than becoming required.
    #[serde(default)]
    pub members: Vec<String>,
}

pub async fn create_email_group(
    Json(req): Json<CreateEmailGroupRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let id = with_kevy(|c| next_id(c, EG_CTR))?;
    let g = serde_json::json!({
        "id": id,
        "address": req.address,
        "domain": req.domain,
        "name": req.name,
        "description": req.description,
        "members": req.members,
        "created_at": now_secs(),
    });
    let payload = serde_json::to_vec(&g).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    with_kevy(move |c| {
        c.hset(
            EG_KEY.as_bytes(),
            &[(id.to_string().as_bytes(), payload.as_slice())],
        )
        .map_err(std::io::Error::other)?;
        Ok(())
    })?;
    Ok(Json(g))
}

pub async fn delete_email_group(Path(id): Path<i64>) -> Result<StatusCode, StatusCode> {
    with_kevy(move |c| {
        c.hdel(EG_KEY.as_bytes(), &[id.to_string().as_bytes()])
            .map_err(std::io::Error::other)?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Admin: greylist local-lists ───────────────────────────────────

const GL_KEY: &str = "admin:greylist:local-lists";
const GL_CTR: &str = "admin:greylist:local-lists:counter";

pub async fn list_greylist_local() -> Result<Json<serde_json::Value>, StatusCode> {
    let vals = with_kevy(|c| hgetall_values(c, GL_KEY))?;
    let items: Vec<serde_json::Value> = vals
        .into_iter()
        .filter_map(|v| serde_json::from_slice(&v).ok())
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

/// A local greylist entry as the client sends it.
///
/// These four fields are the contract the admin UI was built against and
/// the one the monolith implements
/// (`crates/server/src/web/admin/greylist_local.rs`, backed by the
/// `greylist_local_lists` table). This handler was written with
/// `{address_or_domain, list_type}` — invented, overlapping neither — so
/// every create failed with a missing-field 422 and the read side returned
/// records the UI could not display either.
#[derive(Debug, serde::Deserialize)]
pub struct CreateGreylistRequest {
    /// `domain` or `address` — what `value` is.
    pub kind: String,
    /// `whitelist` or `blacklist`.
    pub list: String,
    pub value: String,
    /// Free-text reason. `null` and `""` both mean none; the UI sends
    /// `null` for an untouched field.
    #[serde(default)]
    pub note: Option<String>,
}

pub async fn create_greylist_entry(
    Json(req): Json<CreateGreylistRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let id = with_kevy(|c| next_id(c, GL_CTR))?;
    let g = serde_json::json!({
        "id": id,
        "kind": req.kind,
        "list": req.list,
        "value": req.value,
        "note": req.note,
        "created_at": now_secs(),
    });
    let payload = serde_json::to_vec(&g).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    with_kevy(move |c| {
        c.hset(
            GL_KEY.as_bytes(),
            &[(id.to_string().as_bytes(), payload.as_slice())],
        )
        .map_err(std::io::Error::other)?;
        Ok(())
    })?;
    Ok(Json(g))
}

pub async fn delete_greylist_entry(Path(id): Path<i64>) -> Result<StatusCode, StatusCode> {
    with_kevy(move |c| {
        c.hdel(GL_KEY.as_bytes(), &[id.to_string().as_bytes()])
            .map_err(std::io::Error::other)?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Admin: outbound queue admin view ──────────────────────────────

pub async fn list_admin_queue() -> Result<Json<serde_json::Value>, StatusCode> {
    // Return the last 100 IDs in pending + inflight.
    // v2: read the pending-idx sender actually drains; the legacy
    // `mailrs:outbound:pending` / `mailrs:outbound:inflight` lists have
    // been dead since Phase 8.1 (see stone outbound.rs), so the old
    // read returned only stuck ghosts.
    let ids = with_kevy(|c| {
        let pending = c
            .lrange(b"mailrs:outbound:pending-idx", 0, 99)
            .unwrap_or_default();
        let inflight = c
            .lrange(b"mailrs:outbound:inflight", 0, 99)
            .unwrap_or_default();
        Ok((pending, inflight))
    })?;
    let mut items = Vec::new();
    for (label, list) in [("pending", &ids.0), ("inflight", &ids.1)] {
        for b in list {
            let id_str = String::from_utf8_lossy(b).to_string();
            let key = format!("mailrs:outbound:job:{id_str}");
            let key_c = key.clone();
            let blob = with_kevy(move |c| {
                c.hget(key_c.as_bytes(), b"blob")
                    .map_err(std::io::Error::other)
            })?;
            if let Some(b) = blob
                && let Ok(v) = serde_json::from_slice::<serde_json::Value>(&b)
            {
                let mut item = v;
                if let Some(o) = item.as_object_mut() {
                    o.insert("status".into(), serde_json::Value::String(label.into()));
                }
                items.push(item);
            }
        }
    }
    Ok(Json(serde_json::json!({ "items": items })))
}

// ── Agent: keys + webhooks (per-user) ─────────────────────────────

pub async fn list_agent_keys(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = format!("agent:keys:{user}");
    let vals = with_kevy(move |c| hgetall_values(c, &key))?;
    let items: Vec<serde_json::Value> = vals
        .into_iter()
        .filter_map(|v| serde_json::from_slice(&v).ok())
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

#[derive(Debug, serde::Deserialize)]
pub struct CreateAgentKeyRequest {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub scopes: Vec<String>,
}

pub async fn create_agent_key(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<CreateAgentKeyRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let counter = format!("agent:keys:counter:{user}");
    let id = with_kevy(move |c| next_id(c, &counter))?;
    let secret = format!("mk_{}", random_hex(24));
    let record = serde_json::json!({
        "id": id,
        "name": req.name,
        "scopes": req.scopes,
        "created_at": now_secs(),
        "prefix": &secret[..8],
    });
    let hkey = format!("agent:keys:{user}");
    let payload = serde_json::to_vec(&record).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let secret_c = secret.clone();
    // secret index carries {user, id} so the auth middleware can resolve
    // the owner from a bearer key alone (session.rs agent-key branch).
    // delete_agent_key only removes the hash entry; verification re-checks
    // the hash so a dangling secret index grants nothing.
    let index_payload = serde_json::to_vec(&serde_json::json!({ "user": &user, "id": id }))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    with_kevy(move |c| {
        c.hset(
            hkey.as_bytes(),
            &[(id.to_string().as_bytes(), payload.as_slice())],
        )
        .map_err(std::io::Error::other)?;
        c.set(
            format!("agent:key:secret:{secret_c}").as_bytes(),
            index_payload.as_slice(),
        )
        .map_err(std::io::Error::other)?;
        Ok(())
    })?;
    Ok(Json(serde_json::json!({ "id": id, "secret": secret })))
}

pub async fn delete_agent_key(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("agent:keys:{user}");
    with_kevy(move |c| {
        c.hdel(key.as_bytes(), &[id.to_string().as_bytes()])
            .map_err(std::io::Error::other)?;
        // Also drop the secret index so revoked keys don't accumulate
        // forever. The record doesn't store the secret, so scan the
        // (single-digit-count) index keys for the matching {user,id}.
        let target = serde_json::json!({ "user": user, "id": id });
        for idx_key in c
            .keys(b"agent:key:secret:*")
            .map_err(std::io::Error::other)?
        {
            let Some(raw) = c.get(&idx_key).map_err(std::io::Error::other)? else {
                continue;
            };
            let matches = serde_json::from_slice::<serde_json::Value>(&raw)
                .map(|v| v == target)
                .unwrap_or(false);
            if matches {
                c.del(&[idx_key.as_slice()])
                    .map_err(std::io::Error::other)?;
            }
        }
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/agent/keys:migrate-legacy — one-shot repair for secret
/// indexes written before v2.9.17 (bare-numeric id, no owner). The
/// bare id is a per-user counter, so the owner is recovered by
/// matching the index key's `mk_<8-hex>` prefix against the `prefix`
/// field stored on each user's key records. Idempotent; indexes whose
/// prefix matches no record are dropped (their key was revoked).
pub async fn migrate_legacy_agent_key_indexes(
    Extension(AuthedUser(_user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let (migrated, dropped) = with_kevy(move |c| {
        // prefix -> (user, id) from every user's key records
        let mut by_prefix: std::collections::HashMap<String, (String, i64)> =
            std::collections::HashMap::new();
        for hkey in c.keys(b"agent:keys:*").map_err(std::io::Error::other)? {
            let Ok(hkey_str) = std::str::from_utf8(&hkey) else {
                continue;
            };
            let Some(owner) = hkey_str.strip_prefix("agent:keys:") else {
                continue;
            };
            // skip the counter keys (agent:keys:counter:<user>)
            if owner.starts_with("counter:") {
                continue;
            }
            let owner = owner.to_string();
            for v in hgetall_values(c, hkey_str)? {
                let Ok(rec) = serde_json::from_slice::<serde_json::Value>(&v) else {
                    continue;
                };
                let (Some(prefix), Some(id)) = (rec["prefix"].as_str(), rec["id"].as_i64()) else {
                    continue;
                };
                by_prefix.insert(prefix.to_string(), (owner.clone(), id));
            }
        }
        let mut migrated = 0u32;
        let mut dropped = 0u32;
        for idx_key in c
            .keys(b"agent:key:secret:*")
            .map_err(std::io::Error::other)?
        {
            let Some(raw) = c.get(&idx_key).map_err(std::io::Error::other)? else {
                continue;
            };
            // already-migrated indexes parse as {user,id} — skip
            if serde_json::from_slice::<serde_json::Value>(&raw)
                .map(|v| v.get("user").is_some())
                .unwrap_or(false)
            {
                continue;
            }
            let Ok(idx_str) = std::str::from_utf8(&idx_key) else {
                continue;
            };
            let Some(secret) = idx_str.strip_prefix("agent:key:secret:") else {
                continue;
            };
            let prefix = &secret[..secret.len().min(8)];
            match by_prefix.get(prefix) {
                Some((owner, id)) => {
                    let val = serde_json::json!({ "user": owner, "id": id });
                    c.set(&idx_key, val.to_string().as_bytes())
                        .map_err(std::io::Error::other)?;
                    migrated += 1;
                }
                None => {
                    // no live record carries this prefix — the key was
                    // revoked; drop the dangling index
                    c.del(&[idx_key.as_slice()])
                        .map_err(std::io::Error::other)?;
                    dropped += 1;
                }
            }
        }
        Ok((migrated, dropped))
    })?;
    Ok(Json(
        serde_json::json!({ "migrated": migrated, "dropped": dropped }),
    ))
}

/// The settings page's webhook surface, scoped to the signed-in user.
///
/// Same rows as the admin surface: a user's address *is* their account
/// address, so both now read and write `admin:webhooks:{address}` through
/// `core_sidestate::families::webhooks`. Until 2026-07-31 this wrote a
/// second namespace, `agent:webhooks:{user}`, which meant a subscription
/// created in Settings was invisible to the admin list and vice versa, and
/// the two CRUD implementations had drifted — this one had no
/// `filter_sender` until a fortnight ago and still allocated ids from its
/// own counter, so the two namespaces could hand out the same id.
pub async fn list_agent_webhooks(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let items = with_kevy(move |c| mailrs_core_sidestate::families::webhooks::list(c, &user))?;
    Ok(Json(serde_json::json!({ "items": items })))
}

#[derive(Debug, serde::Deserialize)]
pub struct CreateAgentWebhookRequest {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub event_type: String,
    /// Only fire for mail from this address. Sent by the UI and stored by
    /// both the monolith (`crates/server/src/web/webhook.rs`) and the kevy
    /// family; this handler did not name it, so the value was dropped and
    /// the subscription was created unfiltered — a webhook the user scoped
    /// to one sender was stored as one that matches everything.
    #[serde(default)]
    pub filter_sender: Option<String>,
    /// Only fire for this conversation. Same history as `filter_sender`.
    #[serde(default)]
    pub filter_thread_id: Option<String>,
}

pub async fn create_agent_webhook(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<CreateAgentWebhookRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let w = with_kevy(move |c| {
        mailrs_core_sidestate::families::webhooks::create(
            c,
            mailrs_core_sidestate::families::webhooks::NewWebhook {
                account_address: user,
                url: req.url,
                event_type: req.event_type,
                filter_sender: req.filter_sender,
                filter_thread_id: req.filter_thread_id,
            },
        )
    })?;
    Ok(Json(serde_json::to_value(w).unwrap_or_default()))
}

pub async fn delete_agent_webhook(
    Extension(AuthedUser(_user)): Extension<AuthedUser>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let removed = with_kevy(move |c| mailrs_core_sidestate::families::webhooks::delete(c, id))?;
    match removed {
        true => Ok(StatusCode::NO_CONTENT),
        false => Err(StatusCode::NOT_FOUND),
    }
}
