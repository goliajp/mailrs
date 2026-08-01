//! Apps, agent keys and agent webhooks — the machine-to-machine surface.

use axum::{
    Json,
    extract::{Extension, Path},
    http::StatusCode,
};

use crate::handlers::complete::*;
use crate::handlers::conversations::AuthedUser;

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
