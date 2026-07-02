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
        let mut c = kevy_client::Connection::open(&url)?;
        f(&mut c)
    });
    handle
        .join()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn hgetall_values(c: &mut kevy_client::Connection, key: &str) -> std::io::Result<Vec<Vec<u8>>> {
    let flat = c.hgetall(key.as_bytes())?;
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
    let cur = c.get(counter_key.as_bytes())?;
    let n = match cur {
        Some(bytes) => String::from_utf8_lossy(&bytes).parse::<i64>().unwrap_or(0) + 1,
        None => 1,
    };
    c.set(counter_key.as_bytes(), n.to_string().as_bytes())?;
    Ok(n)
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
        .fast()
        .conversation_categories(&user)
        .await
        .map_err(map_core_err)?;
    let unseen = state
        .fast()
        .unseen_count(&user)
        .await
        .map_err(map_core_err)?;
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
pub async fn oidc_config() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "enabled": false,
        "providers": Vec::<serde_json::Value>::new(),
    }))
}

#[derive(Debug, serde::Deserialize)]
pub struct ForgotPasswordRequest {
    pub address: String,
}

/// POST /api/auth/forgot-password — accept the request, drop a reset
/// token in kevy with a 1-hour TTL. Actual "send an email with the
/// token" step is deferred until we wire an admin notifier. The UI
/// shows a generic "check your inbox" toast regardless, so a 204 is
/// enough to unblock the flow.
pub async fn forgot_password(
    Json(req): Json<ForgotPasswordRequest>,
) -> Result<StatusCode, StatusCode> {
    // Rate-limit — one reset per address per 5 minutes. Without this
    // an attacker can spam the endpoint to churn pwreset:<token>
    // entries and DoS the reset flow.
    let rate_key = format!("pwreset:ratelimit:{}", req.address);
    let rate_key_c = rate_key.clone();
    let now = now_secs();
    let recent = with_kevy(move |c| c.get(rate_key_c.as_bytes()))?
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    if now - recent < 300 {
        // Return 204 anyway so we don't leak "user exists" — the
        // client can't tell whether we actually issued a token.
        return Ok(StatusCode::NO_CONTENT);
    }
    let token = random_hex(24);
    let key = format!("pwreset:{token}");
    let addr = req.address;
    let addr_c = addr.clone();
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
    Ok(StatusCode::NO_CONTENT)
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
    let addr_bytes = with_kevy(move |c| c.hget(format!("pwreset:{token}").as_bytes(), b"address"))?;
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
        .fast()
        .set_account_password(&address, &set_req)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let tok = req.token;
    with_kevy(move |c| {
        c.del(&[format!("pwreset:{tok}").as_bytes()])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/auth/recovery-email — returns the account's recovery
/// email (or null). POST updates it.
pub async fn get_recovery_email(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = format!("mailrs:account:{user}");
    let cur = with_kevy(move |c| c.hget(key.as_bytes(), b"blob"))?;
    let Some(cur) = cur else {
        return Ok(Json(serde_json::json!({ "recovery_email": null })));
    };
    let val: serde_json::Value =
        serde_json::from_slice(&cur).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rec = val
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
        .fast()
        .set_recovery_email(&user, &wire_req)
        .await
        .map_err(|e| StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))?;
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
    let enabled = with_kevy(move |c| c.hget(key_c.as_bytes(), b"enabled"))
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
    let secret = with_kevy(move |c| c.hget(key_r.as_bytes(), b"secret"))?
        .and_then(|v| String::from_utf8(v).ok())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let enabled = with_kevy({
        let k = key.clone();
        move |c| c.hget(k.as_bytes(), b"enabled")
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
    let secret = with_kevy(move |c| c.hget(key_r.as_bytes(), b"secret"))?
        .and_then(|v| String::from_utf8(v).ok())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let enabled_key = key.clone();
    let enabled = with_kevy(move |c| c.hget(enabled_key.as_bytes(), b"enabled"))?
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

// ── /api/mail/keys/status — PGP setup status ──────────────────────

pub async fn keys_status(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = format!("pgp_keys:{user}");
    let flat = with_kevy(move |c| c.hgetall(key.as_bytes()))?;
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
        .fast()
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
    Ok(Json(serde_json::json!({
        "id": w.id,
        "uid": w.uid,
        "sender": w.sender,
        "recipients": w.recipients,
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
    // Push it back onto pending; sender picks it up on the next tick.
    // Kevy-client's LREM isn't stable, so we don't try to remove from
    // inflight explicitly; if the sender was hung it'll still process
    // the item once.
    let id_str = id.to_string();
    with_kevy(move |c| {
        c.lpush(b"mailrs:outbound:pending", &[id_str.as_bytes()])?;
        Ok(())
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
        )?;
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
                c.hdel(APPS_KEY.as_bytes(), &[id.to_string().as_bytes()])?;
                Ok(())
            })?;
            return Ok(StatusCode::NO_CONTENT);
        }
    }
    Err(StatusCode::NOT_FOUND)
}

// ── Admin: audit-log messages/raw + audit/accounts + audit/conversations ────

/// GET /api/admin/audit/accounts — return the registered accounts
/// (from `mailrs:accounts:index`) shaped for the audit panel.
pub async fn audit_accounts(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let members = with_kevy(|c| c.smembers(b"mailrs:accounts:index"))?;
    let items: Vec<serde_json::Value> = members
        .into_iter()
        .filter_map(|v| String::from_utf8(v).ok())
        .map(|addr| serde_json::json!({ "address": addr }))
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
        .fast()
        .list_conversations(&target, &req)
        .await
        .map_err(|e| StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))?;
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
    let flat = with_kevy(move |c| c.hgetall(key.as_bytes()))?;
    if flat.is_empty() {
        return Ok(Json(serde_json::json!({ "thread_id": thread_id, "found": false })));
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
        .fast()
        .list_thread_messages(&q.user, &thread_id)
        .await
        .map_err(|e| StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))?;
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
        .fast()
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
    let key = b"admin:config:smtp".to_vec();
    let raw = with_kevy(move |c| c.get(&key))?;
    if let Some(bytes) = raw
        && let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes)
    {
        return Ok(Json(v));
    }
    Ok(Json(serde_json::json!({
        "host": "",
        "port": 25,
        "starttls": true,
    })))
}

pub async fn set_smtp_config(Json(cfg): Json<serde_json::Value>) -> Result<StatusCode, StatusCode> {
    let payload = serde_json::to_vec(&cfg).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    with_kevy(move |c| {
        c.set(b"admin:config:smtp", payload.as_slice())?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_system_config() -> Result<Json<serde_json::Value>, StatusCode> {
    let flat = with_kevy(|c| c.hgetall(b"admin:system-config"))?;
    let mut items = serde_json::Map::new();
    let mut i = 0;
    while i + 1 < flat.len() {
        let k = String::from_utf8_lossy(&flat[i]).to_string();
        let v = String::from_utf8_lossy(&flat[i + 1]).to_string();
        items.insert(k, serde_json::Value::String(v));
        i += 2;
    }
    Ok(Json(serde_json::Value::Object(items)))
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
        c.hset(b"admin:system-config", &[(k.as_bytes(), v.as_bytes())])?;
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
        )?;
        Ok(())
    })?;
    Ok(Json(g))
}

pub async fn delete_group(Path(id): Path<i64>) -> Result<StatusCode, StatusCode> {
    with_kevy(move |c| {
        c.hdel(GROUPS_KEY.as_bytes(), &[id.to_string().as_bytes()])?;
        c.del(&[
            format!("admin:groups:{id}:permissions").as_bytes(),
            format!("admin:groups:{id}:members").as_bytes(),
        ])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_group_permissions(
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = format!("admin:groups:{id}:permissions");
    let raw = with_kevy(move |c| c.smembers(key.as_bytes()))?;
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
        c.del(&[key.as_bytes()])?;
        let refs: Vec<&[u8]> = req.permissions.iter().map(|s| s.as_bytes()).collect();
        if !refs.is_empty() {
            c.sadd(key.as_bytes(), &refs)?;
        }
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_group_members(
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = format!("admin:groups:{id}:members");
    let raw = with_kevy(move |c| c.smembers(key.as_bytes()))?;
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
        c.sadd(key.as_bytes(), &[addr.as_bytes()])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_group_member(
    Path((id, address)): Path<(i64, String)>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("admin:groups:{id}:members");
    with_kevy(move |c| {
        c.srem(key.as_bytes(), &[address.as_bytes()])?;
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
    #[serde(default)]
    pub name: String,
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
        "name": req.name,
        "members": req.members,
        "created_at": now_secs(),
    });
    let payload = serde_json::to_vec(&g).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    with_kevy(move |c| {
        c.hset(
            EG_KEY.as_bytes(),
            &[(id.to_string().as_bytes(), payload.as_slice())],
        )?;
        Ok(())
    })?;
    Ok(Json(g))
}

pub async fn delete_email_group(Path(id): Path<i64>) -> Result<StatusCode, StatusCode> {
    with_kevy(move |c| {
        c.hdel(EG_KEY.as_bytes(), &[id.to_string().as_bytes()])?;
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

#[derive(Debug, serde::Deserialize)]
pub struct CreateGreylistRequest {
    pub address_or_domain: String,
    pub list_type: String, // "whitelist" | "blacklist"
}

pub async fn create_greylist_entry(
    Json(req): Json<CreateGreylistRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let id = with_kevy(|c| next_id(c, GL_CTR))?;
    let g = serde_json::json!({
        "id": id,
        "address_or_domain": req.address_or_domain,
        "list_type": req.list_type,
        "created_at": now_secs(),
    });
    let payload = serde_json::to_vec(&g).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    with_kevy(move |c| {
        c.hset(
            GL_KEY.as_bytes(),
            &[(id.to_string().as_bytes(), payload.as_slice())],
        )?;
        Ok(())
    })?;
    Ok(Json(g))
}

pub async fn delete_greylist_entry(Path(id): Path<i64>) -> Result<StatusCode, StatusCode> {
    with_kevy(move |c| {
        c.hdel(GL_KEY.as_bytes(), &[id.to_string().as_bytes()])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Admin: outbound queue admin view ──────────────────────────────

pub async fn list_admin_queue() -> Result<Json<serde_json::Value>, StatusCode> {
    // Return the last 100 IDs in pending + inflight.
    let ids = with_kevy(|c| {
        let pending = c
            .lrange(b"mailrs:outbound:pending", 0, 99)
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
            let key = format!("mailrs:outbound:{id_str}");
            let key_c = key.clone();
            let blob = with_kevy(move |c| c.hget(key_c.as_bytes(), b"blob"))?;
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
    with_kevy(move |c| {
        c.hset(
            hkey.as_bytes(),
            &[(id.to_string().as_bytes(), payload.as_slice())],
        )?;
        c.set(
            format!("agent:key:secret:{secret_c}").as_bytes(),
            id.to_string().as_bytes(),
        )?;
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
        c.hdel(key.as_bytes(), &[id.to_string().as_bytes()])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_agent_webhooks(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = format!("agent:webhooks:{user}");
    let vals = with_kevy(move |c| hgetall_values(c, &key))?;
    let items: Vec<serde_json::Value> = vals
        .into_iter()
        .filter_map(|v| serde_json::from_slice(&v).ok())
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

#[derive(Debug, serde::Deserialize)]
pub struct CreateAgentWebhookRequest {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub event_type: String,
}

pub async fn create_agent_webhook(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<CreateAgentWebhookRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let counter = format!("agent:webhooks:counter:{user}");
    let id = with_kevy(move |c| next_id(c, &counter))?;
    let signing_secret = random_hex(24);
    let record = serde_json::json!({
        "id": id,
        "url": req.url,
        "event_type": req.event_type,
        "signing_secret": &signing_secret,
        "created_at": now_secs(),
        "active": true,
    });
    let hkey = format!("agent:webhooks:{user}");
    let payload = serde_json::to_vec(&record).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    with_kevy(move |c| {
        c.hset(
            hkey.as_bytes(),
            &[(id.to_string().as_bytes(), payload.as_slice())],
        )?;
        Ok(())
    })?;
    Ok(Json(record))
}

pub async fn delete_agent_webhook(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("agent:webhooks:{user}");
    with_kevy(move |c| {
        c.hdel(key.as_bytes(), &[id.to_string().as_bytes()])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}
