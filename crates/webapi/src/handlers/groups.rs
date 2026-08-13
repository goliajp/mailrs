//! Permission groups, their members, and email groups.

use axum::{Json, extract::Path, http::StatusCode};

use crate::handlers::complete::*;
use crate::handlers::kevy_util::with_kevy;

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
