//! Agent keys and agent webhooks — the machine-to-machine surface.

use axum::{
    Json,
    extract::{Extension, Path},
    http::StatusCode,
};

use crate::handlers::complete::*;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

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
    _: crate::session::confirm::Confirmed,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<CreateAgentKeyRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    crate::session::validate_agent_scopes(&req.scopes)?;
    let (u, name, scopes) = (user.clone(), req.name.clone(), req.scopes.clone());
    let (id, secret) = with_kevy(move |c| mint_agent_key(c, &u, &name, &scopes))?;
    let detail = format!(
        "id={id} prefix={} name={} scopes={}",
        &secret[..8],
        req.name,
        req.scopes.join(",")
    );
    crate::handlers::audit::record(&user, "agent_key.create", &user, &detail);
    Ok(Json(serde_json::json!({ "id": id, "secret": secret })))
}

/// Write a new key for `user` and return its id and secret. The record in
/// `agent:keys:{user}` is what the key list shows and what revoking
/// removes; the secret index carries `{user, id}` so the auth middleware
/// can resolve the owner from a bearer key alone (session.rs agent-key
/// branch). delete_agent_key only removes the hash entry; verification
/// re-checks the hash so a dangling secret index grants nothing.
pub(crate) fn mint_agent_key(
    c: &mut kevy_client::Connection,
    user: &str,
    name: &str,
    scopes: &[String],
) -> std::io::Result<(i64, String)> {
    let id = next_id(c, &format!("agent:keys:counter:{user}"))?;
    let secret = format!("mk_{}", random_hex(24));
    let record = serde_json::json!({
        "id": id,
        "name": name,
        "scopes": scopes,
        "created_at": now_secs(),
        "prefix": &secret[..8],
    });
    let index = serde_json::json!({ "user": user, "id": id });
    c.hset(
        format!("agent:keys:{user}").as_bytes(),
        &[(id.to_string().as_bytes(), record.to_string().as_bytes())],
    )?;
    c.set(
        crate::agent_key_index::index_key(&secret).as_bytes(),
        index.to_string().as_bytes(),
    )?;
    Ok((id, secret))
}

pub async fn delete_agent_key(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let u = user.clone();
    let record = with_kevy(move |c| revoke_agent_key(c, &u, id))?;
    // the key list's prefix, so the row names which key it was
    if let Some(raw) = record {
        let prefix = serde_json::from_slice::<serde_json::Value>(&raw)
            .ok()
            .and_then(|v| v["prefix"].as_str().map(str::to_string))
            .unwrap_or_default();
        let detail = format!("id={id} prefix={prefix}");
        crate::handlers::audit::record(&user, "agent_key.revoke", &user, &detail);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Remove `user`'s key `id`; the record it had, or `None` when there was
/// none. Without its record the key resolves to nobody at once.
pub(crate) fn revoke_agent_key(
    c: &mut kevy_client::Connection,
    user: &str,
    id: i64,
) -> std::io::Result<Option<Vec<u8>>> {
    let key = format!("agent:keys:{user}");
    let field = id.to_string();
    let record = c.hget(key.as_bytes(), field.as_bytes())?;
    c.hdel(key.as_bytes(), &[field.as_bytes()])?;
    c.hdel(
        crate::agent_key_usage::hash_key(user).as_bytes(),
        &[field.as_bytes()],
    )?;
    // Also drop the secret index so revoked keys don't accumulate
    // forever. The record doesn't store the secret, so scan the
    // (single-digit-count) index keys for the matching {user,id}.
    let target = serde_json::json!({ "user": user, "id": id });
    for idx_key in c.keys(crate::agent_key_index::PATTERN)? {
        let Some(raw) = c.get(&idx_key)? else {
            continue;
        };
        let matches = serde_json::from_slice::<serde_json::Value>(&raw)
            .map(|v| v == target)
            .unwrap_or(false);
        if matches {
            c.del(&[idx_key.as_slice()])?;
        }
    }
    Ok(record)
}

/// The settings page's webhook surface, scoped to the signed-in user.
///
/// Same rows as the admin surface: a user's address *is* their account
/// address, so both now read and write `admin:webhooks:{address}` through
/// `core_sidestate::families::webhooks`. A separate `agent:webhooks:{user}`
/// namespace once made the two lists disjoint and their ids collide.
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
    _: crate::session::confirm::Confirmed,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<CreateAgentWebhookRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let u = user.clone();
    let w = with_kevy(move |c| {
        mailrs_core_sidestate::families::webhooks::create(
            c,
            mailrs_core_sidestate::families::webhooks::NewWebhook {
                account_address: u,
                url: req.url,
                event_type: req.event_type,
                filter_sender: req.filter_sender,
                filter_thread_id: req.filter_thread_id,
            },
        )
    })?;
    // the id only: a webhook URL can carry the receiver's token
    crate::handlers::audit::record(&user, "webhook.create", &user, &format!("id={}", w.id));
    Ok(Json(serde_json::to_value(w).unwrap_or_default()))
}

pub async fn delete_agent_webhook(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let u = user.clone();
    let removed =
        with_kevy(move |c| mailrs_core_sidestate::families::webhooks::delete_owned(c, id, &u))?;
    match removed {
        true => {
            crate::handlers::audit::record(&user, "webhook.delete", &user, &format!("id={id}"));
            Ok(StatusCode::NO_CONTENT)
        }
        false => Err(StatusCode::NOT_FOUND),
    }
}
