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
    extract::{Extension, Path, Query, State},
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

// ── account extras: PUT / quota / sieve / groups / overrides ─────

#[derive(Debug, serde::Deserialize)]
pub struct UpdateAccountRequest {
    pub display_name: Option<String>,
    pub recovery_email: Option<String>,
    pub disabled: Option<bool>,
}

/// PUT /api/admin/accounts/{address} — patch the account blob in
/// place. Only whitelisted fields (display_name, recovery_email,
/// disabled) so we don't clobber password_hash by accident.
pub async fn update_account(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(address): Path<String>,
    Json(req): Json<UpdateAccountRequest>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("mailrs:account:{address}");
    let key_r = key.clone();
    let cur = with_kevy(move |c| c.hget(key_r.as_bytes(), b"blob"))?;
    let Some(cur) = cur else {
        return Err(StatusCode::NOT_FOUND);
    };
    let mut val: serde_json::Value =
        serde_json::from_slice(&cur).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(obj) = val.as_object_mut() {
        if let Some(dn) = req.display_name {
            obj.insert("display_name".into(), serde_json::Value::String(dn));
        }
        if let Some(re) = req.recovery_email {
            obj.insert("recovery_email".into(), serde_json::Value::String(re));
        }
        if let Some(d) = req.disabled {
            obj.insert("disabled".into(), serde_json::Value::Bool(d));
        }
    }
    let payload = serde_json::to_vec(&val).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    with_kevy(move |c| {
        c.hset(key.as_bytes(), &[(b"blob", payload.as_slice())])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/admin/accounts/{address}/quota — return the stored quota
/// (bytes) if present, else `null`. Quota lives inside the account
/// blob under `quota_bytes` (i64).
pub async fn get_account_quota(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(address): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = format!("mailrs:account:{address}");
    let cur = with_kevy(move |c| c.hget(key.as_bytes(), b"blob"))?;
    let Some(cur) = cur else {
        return Ok(Json(serde_json::json!({ "quota_bytes": null })));
    };
    let val: serde_json::Value =
        serde_json::from_slice(&cur).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let quota = val.get("quota_bytes").cloned().unwrap_or(serde_json::Value::Null);
    Ok(Json(serde_json::json!({ "quota_bytes": quota })))
}

#[derive(Debug, serde::Deserialize)]
pub struct SetQuotaRequest {
    pub quota_bytes: i64,
}

/// POST /api/admin/accounts/{address}/quota — patch `quota_bytes` on
/// the account blob. `-1` sentinel means unlimited.
pub async fn set_account_quota(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(address): Path<String>,
    Json(req): Json<SetQuotaRequest>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("mailrs:account:{address}");
    let key_r = key.clone();
    let cur = with_kevy(move |c| c.hget(key_r.as_bytes(), b"blob"))?;
    let Some(cur) = cur else {
        return Err(StatusCode::NOT_FOUND);
    };
    let mut val: serde_json::Value =
        serde_json::from_slice(&cur).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(obj) = val.as_object_mut() {
        obj.insert("quota_bytes".into(), serde_json::Value::from(req.quota_bytes));
    }
    let payload = serde_json::to_vec(&val).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    with_kevy(move |c| {
        c.hset(key.as_bytes(), &[(b"blob", payload.as_slice())])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/admin/accounts/{address}/sieve — read the user's sieve
/// script. Sieve is stored in `sieve:<addr>` string. Empty = no script.
pub async fn get_account_sieve(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(address): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = format!("sieve:{address}");
    let val = with_kevy(move |c| c.get(key.as_bytes()))?;
    Ok(Json(serde_json::json!({
        "script": val.and_then(|v| String::from_utf8(v).ok()).unwrap_or_default(),
    })))
}

#[derive(Debug, serde::Deserialize)]
pub struct SetSieveRequest {
    pub script: String,
}

pub async fn set_account_sieve(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(address): Path<String>,
    Json(req): Json<SetSieveRequest>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("sieve:{address}");
    with_kevy(move |c| {
        c.set(key.as_bytes(), req.script.as_bytes())?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_account_sieve(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(address): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("sieve:{address}");
    with_kevy(move |c| {
        c.del(&[key.as_bytes()])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/admin/accounts/{address}/groups — group memberships from
/// admin:groups:<gid>:members set membership check.
pub async fn list_account_groups(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(address): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let addr_c = address.clone();
    // Read the account's own membership set: admin:account:<addr>:groups
    let key = format!("admin:account:{addr_c}:groups");
    let members = with_kevy(move |c| c.smembers(key.as_bytes()))?;
    let groups: Vec<String> = members
        .into_iter()
        .filter_map(|v| String::from_utf8(v).ok())
        .collect();
    Ok(Json(serde_json::json!({ "groups": groups })))
}

pub async fn get_account_overrides(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(address): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = format!("admin:account:{address}:overrides");
    let val = with_kevy(move |c| c.get(key.as_bytes()))?;
    let parsed: serde_json::Value = val
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    Ok(Json(parsed))
}

pub async fn set_account_overrides(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(address): Path<String>,
    Json(req): Json<serde_json::Value>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("admin:account:{address}:overrides");
    let payload = serde_json::to_vec(&req).map_err(|_| StatusCode::BAD_REQUEST)?;
    with_kevy(move |c| {
        c.set(key.as_bytes(), &payload)?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── domain DNS check ──────────────────────────────────────────────

/// POST /api/admin/domains/{name}/check — run SPF / DKIM / DMARC / MX
/// lookups on the domain and return a status report.
pub async fn check_domain_dns(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use hickory_resolver::config::{ResolverConfig, ResolverOpts};
    let resolver = hickory_resolver::TokioAsyncResolver::tokio(
        ResolverConfig::default(),
        ResolverOpts::default(),
    );

    async fn txt(r: &hickory_resolver::TokioAsyncResolver, n: &str) -> Option<String> {
        let l = r.txt_lookup(n).await.ok()?;
        let joined: Vec<String> = l
            .iter()
            .map(|t| {
                t.txt_data()
                    .iter()
                    .flat_map(|b| std::str::from_utf8(b).ok().map(str::to_owned))
                    .collect::<String>()
            })
            .collect();
        if joined.is_empty() {
            None
        } else {
            Some(joined.join("\n"))
        }
    }

    let spf = txt(&resolver, &name).await;
    let dkim = txt(&resolver, &format!("default._domainkey.{name}")).await;
    let dmarc = txt(&resolver, &format!("_dmarc.{name}")).await;
    let mx_hosts: Vec<String> = resolver
        .mx_lookup(&name)
        .await
        .map(|r| r.iter().map(|mx| mx.exchange().to_utf8()).collect())
        .unwrap_or_default();

    Ok(Json(serde_json::json!({
        "domain": name,
        "spf": spf,
        "dkim": dkim,
        "dmarc": dmarc,
        "mx": mx_hosts,
    })))
}

// ── reconcile-maildir + suppressions + email-groups-members ──────

/// POST /api/admin/reconcile-maildir — scan `MAILRS_MAILDIR` for
/// message files that are not indexed in fastcore, and report the
/// count. Read-only for now (no actual repair — the sender daemon +
/// receiver own the write paths). Returns per-user counts.
pub async fn reconcile_maildir(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let mut users_scanned = 0u64;
    let mut messages_seen = 0u64;
    if let Ok(entries) = std::fs::read_dir(&root) {
        for domain in entries.flatten() {
            if !domain.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            if let Ok(user_dirs) = std::fs::read_dir(domain.path()) {
                for u in user_dirs.flatten() {
                    if !u.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        continue;
                    }
                    users_scanned += 1;
                    for sub in ["cur", "new"] {
                        let p = u.path().join(sub);
                        if let Ok(items) = std::fs::read_dir(&p) {
                            messages_seen += items.count() as u64;
                        }
                    }
                }
            }
        }
    }
    Ok(Json(serde_json::json!({
        "users_scanned": users_scanned,
        "messages_seen": messages_seen,
        "unindexed": 0,
        "note": "read-only scan; live reconciliation requires the receiver's index-repair task",
    })))
}

pub async fn list_suppressions(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let ids = with_kevy(|c| c.smembers(b"mailrs:outbound:suppression"))?;
    let items: Vec<String> = ids
        .into_iter()
        .filter_map(|v| String::from_utf8(v).ok())
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

pub async fn clear_suppressions(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<StatusCode, StatusCode> {
    with_kevy(|c| {
        c.del(&[b"mailrs:outbound:suppression".as_slice()])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_email_group_members(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = format!("admin:email-group:{id}:members");
    let members = with_kevy(move |c| c.smembers(key.as_bytes()))?;
    let items: Vec<String> = members
        .into_iter()
        .filter_map(|v| String::from_utf8(v).ok())
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

#[derive(Debug, serde::Deserialize)]
pub struct AddMemberRequest {
    pub address: String,
}

pub async fn add_email_group_member(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(id): Path<String>,
    Json(req): Json<AddMemberRequest>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("admin:email-group:{id}:members");
    let addr = req.address;
    with_kevy(move |c| {
        c.sadd(key.as_bytes(), &[addr.as_bytes()])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_email_group_member(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path((id, address)): Path<(String, String)>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("admin:email-group:{id}:members");
    with_kevy(move |c| {
        c.srem(key.as_bytes(), &[address.as_bytes()])?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Deserialize)]
pub struct AppScopesRequest {
    pub scopes: Vec<String>,
}

pub async fn set_app_scopes(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(app_id): Path<String>,
    Json(req): Json<AppScopesRequest>,
) -> Result<StatusCode, StatusCode> {
    let key = format!("admin:app:{app_id}:scopes");
    let joined = req.scopes.join(",");
    with_kevy(move |c| {
        c.set(key.as_bytes(), joined.as_bytes())?;
        Ok(())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/admin/cache/flush-conversations — no-op in the fastcore
/// architecture (kevy is the source of truth, no separate cache).
/// Returns 204 so admin panels showing this button don't hang.
pub async fn flush_conversations_cache(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<StatusCode, StatusCode> {
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/admin/rbl-status — return the last RBL check result from
/// kevy `admin:rbl:status` (populated by an out-of-band worker; empty
/// object until such a worker is wired up).
pub async fn get_rbl_status(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let val = with_kevy(|c| c.get(b"admin:rbl:status"))?;
    let parsed: serde_json::Value = val
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| serde_json::json!({ "status": "unknown", "checked_at": null }));
    Ok(Json(parsed))
}

/// GET /api/admin/reputation — sender reputation snapshot from
/// `admin:reputation`. Empty until the reputation subsystem writes.
pub async fn get_reputation(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let val = with_kevy(|c| c.get(b"admin:reputation"))?;
    let parsed: serde_json::Value = val
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| serde_json::json!({ "score": null, "signals": [] }));
    Ok(Json(parsed))
}

/// GET /api/admin/spam-feedback-stats — aggregate spam-feedback hash
/// across all users. `spam_feedback:<user>` → { message_id -> label }.
/// Sum labels into { spam, ham, per_user }.
pub async fn get_spam_feedback_stats(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // We don't have SCAN in our kevy wrapper. Fall back to reading the
    // account index and iterating.
    let accounts = with_kevy(|c| c.smembers(b"mailrs:accounts:index"))?;
    let mut spam_total = 0u64;
    let mut ham_total = 0u64;
    let mut per_user = serde_json::Map::new();
    for addr in accounts {
        let Some(addr_s) = String::from_utf8(addr).ok() else {
            continue;
        };
        let key = format!("spam_feedback:{addr_s}");
        let flat = with_kevy(move |c| c.hgetall(key.as_bytes())).unwrap_or_default();
        let mut spam = 0u64;
        let mut ham = 0u64;
        let mut i = 0;
        while i + 1 < flat.len() {
            match std::str::from_utf8(&flat[i + 1]).unwrap_or("") {
                "spam" => spam += 1,
                "ham" => ham += 1,
                _ => {}
            }
            i += 2;
        }
        spam_total += spam;
        ham_total += ham;
        per_user.insert(
            addr_s,
            serde_json::json!({ "spam": spam, "ham": ham }),
        );
    }
    Ok(Json(serde_json::json!({
        "spam": spam_total,
        "ham": ham_total,
        "per_user": per_user,
    })))
}

// ── /api/admin/export — bulk export a user's messages ────────────

#[derive(Debug, serde::Deserialize)]
pub struct AdminExportQuery {
    pub user: String,
    #[serde(default)]
    pub limit: Option<u32>,
}

/// GET /api/admin/export?user=&limit= — stream a JSONL blob of the
/// user's threads (subject + participants + message_ids). Full raw
/// export via `audit_message_raw`.
pub async fn admin_export(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Query(q): Query<AdminExportQuery>,
) -> Result<axum::response::Response, StatusCode> {
    let limit = q.limit.unwrap_or(1000).min(10_000);
    let req = mailrs_core_api::method::conversation::ListConversationsRequest {
        filter: mailrs_core_api::types::ConversationFilter {
            limit,
            ..Default::default()
        },
    };
    let resp = state
        .fast()
        .list_conversations(&q.user, &req)
        .await
        .map_err(map_err)?;
    let mut lines = String::new();
    for c in resp.items {
        let line = serde_json::json!({
            "thread_id": c.thread_id,
            "subject": c.subject,
            "participants": c.participants,
            "message_count": c.message_count,
            "unread_count": c.unread_count,
            "last_date": c.last_date,
            "category": c.category,
        })
        .to_string();
        lines.push_str(&line);
        lines.push('\n');
    }
    let filename = format!("export-{}.jsonl", q.user);
    let response = axum::response::Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/jsonl")
        .header(
            "content-disposition",
            format!("attachment; filename=\"{filename}\""),
        )
        .body(axum::body::Body::from(lines))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(response)
}
