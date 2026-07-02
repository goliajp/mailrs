//! `/api/admin/*` REST handlers — fastcore-native.
//!
//! - Accounts routes proxy to fastcore RPC (accounts live in kevy).
//! - Aliases / domains / webhooks / audit are stored in the shared
//!   network kevy under `admin:*` keys.
//!
//! Zero spg touch.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Query, State},
    http::StatusCode,
};

use mailrs_core_api::method::admin as wire;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

fn map_err(e: mailrs_core_api::error::CoreApiError) -> StatusCode {
    StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

/// Blocking kevy helper. Same pattern as `handlers::prefs::with_kevy`.
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

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Next id from a `<hash>:counter` string.
fn next_id(c: &mut kevy_client::Connection, counter_key: &str) -> std::io::Result<i64> {
    let cur = c.get(counter_key.as_bytes())?;
    let n = match cur {
        Some(bytes) => String::from_utf8_lossy(&bytes).parse::<i64>().unwrap_or(0) + 1,
        None => 1,
    };
    c.set(counter_key.as_bytes(), n.to_string().as_bytes())?;
    Ok(n)
}

fn hgetall_values(c: &mut kevy_client::Connection, key: &str) -> std::io::Result<Vec<Vec<u8>>> {
    let flat = c.hgetall(key.as_bytes())?;
    Ok(flat
        .into_iter()
        .enumerate()
        .filter_map(|(i, v)| if i % 2 == 1 { Some(v) } else { None })
        .collect())
}

// ── accounts (via fastcore RPC) ────────────────────────────────────

/// GET /api/admin/accounts
pub async fn list_accounts(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<wire::AccountListResponse>, StatusCode> {
    state
        .fast()
        .list_accounts()
        .await
        .map(Json)
        .map_err(map_err)
}

/// POST /api/admin/accounts — provision a new account. Writes an
/// AccountWithHashWire blob into fastcore-side kevy (via network kevy,
/// same key shape `mailrs:account:<addr>`) plus an empty
/// EffectivePermissionsResponse. Password is argon2-hashed here.
pub async fn add_account(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Json(req): Json<wire::AddAccountRequest>,
) -> Result<StatusCode, StatusCode> {
    use argon2::{
        Argon2,
        password_hash::{PasswordHasher, SaltString, rand_core::OsRng as ArgonRng},
    };
    let salt = SaltString::generate(&mut ArgonRng);
    let hash = Argon2::default()
        .hash_password(req.password.as_bytes(), &salt)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .to_string();
    let domain = req
        .address
        .split_once('@')
        .map(|(_, d)| d.to_string())
        .unwrap_or_default();
    let blob = serde_json::json!({
        "address": &req.address,
        "domain": domain,
        "display_name": req.display_name,
        "active": true,
        "created_at": now_secs(),
        "quota_bytes": 10_737_418_240i64,
        "recovery_email": null,
        "password_hash": hash,
    });
    let payload = serde_json::to_vec(&blob).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let addr_c = req.address.clone();
    with_kevy(move |c| {
        let key = format!("mailrs:account:{addr_c}");
        c.hset(key.as_bytes(), &[(b"blob", payload.as_slice())])?;
        c.sadd(b"mailrs:accounts:index", &[addr_c.as_bytes()])?;
        Ok(())
    })?;
    // Empty perms blob — admin bootstraps their own perms via
    // /api/admin/groups later.
    let perms = serde_json::json!({
        "address": &req.address,
        "permissions": Vec::<String>::new(),
        "groups": Vec::<serde_json::Value>::new(),
        "is_super": false,
        "send_as": Vec::<String>::new(),
    });
    let perms_payload =
        serde_json::to_vec(&perms).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let addr_c2 = req.address.clone();
    with_kevy(move |c| {
        let key = format!("mailrs:account:{addr_c2}:perms");
        c.set(key.as_bytes(), perms_payload.as_slice())?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/admin/accounts/{address} — remove account entries from
/// kevy. Does not touch maildir on disk — the operator is responsible
/// for cleaning that up if they want a hard delete.
pub async fn remove_account(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    axum::extract::Path(address): axum::extract::Path<String>,
) -> Result<StatusCode, StatusCode> {
    let addr = address.clone();
    with_kevy(move |c| {
        c.del(&[
            format!("mailrs:account:{addr}").as_bytes(),
            format!("mailrs:account:{addr}:perms").as_bytes(),
        ])?;
        c.srem(b"mailrs:accounts:index", &[addr.as_bytes()])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── aliases (network kevy) ─────────────────────────────────────────

const ALIAS_KEY: &str = "admin:aliases";
const ALIAS_CTR: &str = "admin:aliases:counter";

/// GET /api/admin/aliases
pub async fn list_aliases(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<wire::AliasListResponse>, StatusCode> {
    let vals = with_kevy(|c| hgetall_values(c, ALIAS_KEY))?;
    let items: Vec<wire::AliasWire> = vals
        .into_iter()
        .filter_map(|v| serde_json::from_slice(&v).ok())
        .collect();
    Ok(Json(wire::AliasListResponse { items }))
}

/// POST /api/admin/aliases
pub async fn add_alias(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Json(req): Json<wire::AddAliasRequest>,
) -> Result<Json<wire::AddAliasResponse>, StatusCode> {
    let id = with_kevy(|c| next_id(c, ALIAS_CTR))?;
    let alias = wire::AliasWire {
        id,
        source_address: req.source_address,
        target_address: req.target_address,
        domain: req.domain,
        alias_type: req.alias_type,
        active: true,
        created_at: now_secs(),
    };
    let json = serde_json::to_vec(&alias).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    with_kevy(move |c| {
        c.hset(
            ALIAS_KEY.as_bytes(),
            &[(id.to_string().as_bytes(), json.as_slice())],
        )?;
        Ok(())
    })?;
    Ok(Json(wire::AddAliasResponse { id }))
}

/// DELETE /api/admin/aliases/{id}
pub async fn remove_alias(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Result<StatusCode, StatusCode> {
    with_kevy(move |c| {
        c.hdel(ALIAS_KEY.as_bytes(), &[id.to_string().as_bytes()])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── domains (network kevy) ─────────────────────────────────────────

const DOMAIN_KEY: &str = "admin:domains";

/// GET /api/admin/domains
pub async fn list_domains(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<wire::DomainListResponse>, StatusCode> {
    let vals = with_kevy(|c| hgetall_values(c, DOMAIN_KEY))?;
    let items: Vec<wire::DomainWire> = vals
        .into_iter()
        .filter_map(|v| serde_json::from_slice(&v).ok())
        .collect();
    Ok(Json(wire::DomainListResponse { items }))
}

#[derive(Debug, serde::Deserialize)]
pub struct AddDomainBody {
    pub name: String,
}

/// POST /api/admin/domains
pub async fn add_domain(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Json(req): Json<AddDomainBody>,
) -> Result<StatusCode, StatusCode> {
    let d = wire::DomainWire {
        name: req.name.clone(),
        created_at: now_secs(),
    };
    let json = serde_json::to_vec(&d).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let name = req.name;
    with_kevy(move |c| {
        c.hset(DOMAIN_KEY.as_bytes(), &[(name.as_bytes(), json.as_slice())])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/admin/domains/{name}
pub async fn remove_domain(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Result<StatusCode, StatusCode> {
    with_kevy(move |c| {
        c.hdel(DOMAIN_KEY.as_bytes(), &[name.as_bytes()])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── webhooks (network kevy) ────────────────────────────────────────

const WEBHOOK_KEY_PREFIX: &str = "admin:webhooks:";
const WEBHOOK_CTR: &str = "admin:webhooks:counter";

/// POST /api/admin/webhook-subscriptions
pub async fn create_webhook(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Json(req): Json<wire::CreateWebhookRequest>,
) -> Result<Json<wire::CreateWebhookResponse>, StatusCode> {
    use base64::Engine as _;
    let id = with_kevy(|c| next_id(c, WEBHOOK_CTR))?;
    let mut secret_bytes = [0u8; 32];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut secret_bytes);
    let signing_secret = base64::engine::general_purpose::STANDARD.encode(secret_bytes);
    let w = wire::WebhookSubWire {
        id,
        account_address: req.account_address.clone(),
        url: req.url,
        event_type: req.event_type,
        filter_sender: req.filter_sender,
        filter_thread_id: req.filter_thread_id,
        signing_secret: signing_secret.clone(),
        active: true,
        created_at: now_secs(),
    };
    let json = serde_json::to_vec(&w).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let key = format!("{WEBHOOK_KEY_PREFIX}{}", req.account_address);
    with_kevy(move |c| {
        c.hset(
            key.as_bytes(),
            &[(id.to_string().as_bytes(), json.as_slice())],
        )?;
        Ok(())
    })?;
    Ok(Json(wire::CreateWebhookResponse { id, signing_secret }))
}

/// GET /api/admin/accounts/{address}/webhook-subscriptions
pub async fn list_webhooks(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    axum::extract::Path(address): axum::extract::Path<String>,
) -> Result<Json<wire::WebhookListResponse>, StatusCode> {
    let key = format!("{WEBHOOK_KEY_PREFIX}{address}");
    let vals = with_kevy(move |c| hgetall_values(c, &key))?;
    let items: Vec<wire::WebhookSubWire> = vals
        .into_iter()
        .filter_map(|v| serde_json::from_slice(&v).ok())
        .collect();
    Ok(Json(wire::WebhookListResponse { items }))
}

/// DELETE /api/admin/webhook-subscriptions/{id}
pub async fn delete_webhook(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Result<StatusCode, StatusCode> {
    // Webhook is keyed by account — scan by iterating known keys.
    // Cheap: single-user or few-user deployments dominate. If it grows,
    // we can index (id -> account) separately.
    let id_str = id.to_string();
    with_kevy(move |c| {
        // simple scan — try all known accounts (SMEMBERS)
        let addrs = c.smembers(b"mailrs:accounts:index").unwrap_or_default();
        for addr_bytes in addrs {
            if let Ok(addr) = String::from_utf8(addr_bytes) {
                let key = format!("{WEBHOOK_KEY_PREFIX}{addr}");
                c.hdel(key.as_bytes(), &[id_str.as_bytes()])?;
            }
        }
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── audit log (network kevy list) ──────────────────────────────────

const AUDIT_KEY: &str = "admin:audit_log";

#[derive(Debug, serde::Deserialize)]
pub struct AuditQuery {
    #[serde(default = "default_audit_limit")]
    pub limit: u32,
}

fn default_audit_limit() -> u32 {
    100
}

/// GET /api/admin/audit-log
pub async fn list_audit_log(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Query(q): Query<AuditQuery>,
) -> Result<Json<wire::AuditListResponse>, StatusCode> {
    let limit = q.limit as i64;
    let entries = with_kevy(move |c| c.lrange(AUDIT_KEY.as_bytes(), 0, limit - 1))?;
    let items: Vec<wire::AuditRowWire> = entries
        .into_iter()
        .filter_map(|v| serde_json::from_slice(&v).ok())
        .collect();
    Ok(Json(wire::AuditListResponse { items }))
}
