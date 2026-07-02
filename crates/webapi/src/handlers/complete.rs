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
    let token = random_hex(24);
    let key = format!("pwreset:{token}");
    let addr = req.address;
    let addr_c = addr.clone();
    with_kevy(move |c| {
        c.hset(
            key.as_bytes(),
            &[
                (b"address" as &[u8], addr_c.as_bytes()),
                (b"issued_at", now_secs().to_string().as_bytes()),
            ],
        )?;
        // TTL best-effort — expire in 3600 s. If the kevy build lacks
        // hexpire, the entry just lives longer; not a security issue
        // for a token that's still gated by "must know the token".
        let _ = c.expire(
            format!("pwreset:{token}").as_bytes(),
            std::time::Duration::from_secs(3600),
        );
        // Also index address → latest token so the "check your inbox"
        // debug page can retrieve it. Fine for single-tenant use.
        c.set(
            format!("pwreset_by_addr:{addr}").as_bytes(),
            token.as_bytes(),
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

/// POST /api/auth/reset-password — verify the token then update the
/// argon2 hash inside the kevy account blob.
pub async fn reset_password(
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
    // Update the account blob in place: HGET blob, patch password_hash,
    // HSET back. Fastcore's kevy is authoritative.
    let addr_c = address.clone();
    let key = format!("mailrs:account:{addr_c}");
    let key_c = key.clone();
    let cur = with_kevy(move |c| c.hget(key_c.as_bytes(), b"blob"))?;
    let Some(cur) = cur else {
        return Err(StatusCode::NOT_FOUND);
    };
    let mut val: serde_json::Value =
        serde_json::from_slice(&cur).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(obj) = val.as_object_mut() {
        obj.insert("password_hash".to_string(), serde_json::Value::String(hash));
    }
    let payload = serde_json::to_vec(&val).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let tok = req.token;
    with_kevy(move |c| {
        c.hset(key.as_bytes(), &[(b"blob", payload.as_slice())])?;
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
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<SetRecoveryEmailRequest>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("mailrs:account:{user}");
    let key_c = key.clone();
    let cur = with_kevy(move |c| c.hget(key_c.as_bytes(), b"blob"))?;
    let mut val: serde_json::Value = cur
        .as_deref()
        .and_then(|b| serde_json::from_slice(b).ok())
        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
    if let Some(obj) = val.as_object_mut() {
        obj.insert(
            "recovery_email".to_string(),
            match req.recovery_email {
                Some(s) => serde_json::Value::String(s),
                None => serde_json::Value::Null,
            },
        );
    }
    let payload = serde_json::to_vec(&val).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    with_kevy(move |c| {
        c.hset(key.as_bytes(), &[(b"blob", payload.as_slice())])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/auth/totp/status — 2FA enrollment status. Always disabled
/// for now; enabling requires a proper enrollment flow (secret + QR +
/// verify). Returns the exact shape the settings page reads.
pub async fn totp_status(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "enabled": false,
        "address": user,
    }))
}

pub async fn totp_setup() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "secret": null,
        "qr_svg": null,
    }))
}

pub async fn totp_enable(Json(_req): Json<serde_json::Value>) -> Result<StatusCode, StatusCode> {
    Err(StatusCode::NOT_IMPLEMENTED)
}

pub async fn totp_disable(Json(_req): Json<serde_json::Value>) -> Result<StatusCode, StatusCode> {
    Ok(StatusCode::NO_CONTENT)
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
    let id = with_kevy(|c| next_id(c, APPS_CTR))?;
    let app_id = format!("app_{id}");
    let secret = random_hex(32);
    let blob = serde_json::json!({
        "id": id,
        "app_id": app_id,
        "name": req.name,
        "scopes": req.scopes,
        "created_at": now_secs(),
    });
    let payload = serde_json::to_vec(&blob).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    with_kevy(move |c| {
        c.hset(
            APPS_KEY.as_bytes(),
            &[(id.to_string().as_bytes(), payload.as_slice())],
        )?;
        Ok(())
    })?;
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

pub async fn audit_accounts() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "items": [] }))
}
pub async fn audit_conversations() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "items": [] }))
}
pub async fn audit_conversation_detail(
    Path(_thread_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    Ok(Json(serde_json::json!({ "items": [] })))
}
pub async fn audit_message_raw(
    Path(_uid): Path<u32>,
) -> Result<axum::response::Response, StatusCode> {
    // Empty payload — same effect as "no cached raw for this message".
    axum::response::Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(axum::body::Body::empty())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
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
