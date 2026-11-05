//! Permission groups, their members, and email groups.

use axum::{
    Json,
    extract::{Extension, Path},
    http::StatusCode,
};

use crate::handlers::audit::record;
use crate::handlers::conversations::AuthedUser;

use crate::handlers::complete::*;
use crate::handlers::kevy_util::with_kevy;
use mailrs_core_sidestate::families::email_groups::{
    COUNTER_KEY as EG_CTR, GROUPS_KEY as EG_KEY, members_key as email_group_members_key,
};
use mailrs_core_sidestate::families::permission_groups::{
    self as groups, GROUPS_KEY, members_key, permissions_key,
};

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
    _: crate::session::confirm::Confirmed,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Json(req): Json<CreateGroupRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let name = req.name.clone();
    let row = with_kevy(move |c| Ok(groups::create(c, &req.name, &req.description, now_secs())?))?;
    record(&actor, "group.create", &row["id"].to_string(), &name);
    Ok(Json(row))
}

/// DELETE /api/admin/groups/{id} — 409 for a builtin group.
pub async fn delete_group(
    _: crate::session::confirm::Confirmed,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    match with_kevy(move |c| Ok(groups::delete(c, id)?))? {
        true => {
            record(&actor, "group.delete", &id.to_string(), "");
            Ok(StatusCode::NO_CONTENT)
        }
        false => Err(StatusCode::CONFLICT),
    }
}

pub async fn get_group_permissions(
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = permissions_key(id);
    let raw = with_kevy(move |c| c.smembers(key.as_bytes()).map_err(std::io::Error::from))?;
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
    _: crate::session::confirm::Confirmed,
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::WebState>>,
    axum::extract::Extension(crate::handlers::conversations::AuthedUser(actor)): axum::extract::Extension<
        crate::handlers::conversations::AuthedUser,
    >,
    Path(id): Path<i64>,
    Json(req): Json<SetGroupPermissionsRequest>,
) -> Result<StatusCode, StatusCode> {
    // the group's members get these: only what the writer could grant
    crate::handlers::grants::require_may_grant(&state, &actor, &req.permissions).await?;
    let detail = req.permissions.join(",");
    with_kevy(move |c| Ok(groups::set_permissions(c, id, &req.permissions)?))?;
    record(&actor, "group.permissions_set", &id.to_string(), &detail);
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_group_members(
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = members_key(id);
    let raw = with_kevy(move |c| c.smembers(key.as_bytes()).map_err(std::io::Error::from))?;
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
    _: crate::session::confirm::Confirmed,
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::WebState>>,
    axum::extract::Extension(crate::handlers::conversations::AuthedUser(actor)): axum::extract::Extension<
        crate::handlers::conversations::AuthedUser,
    >,
    Path(id): Path<i64>,
    Json(req): Json<AddGroupMemberRequest>,
) -> Result<StatusCode, StatusCode> {
    // a new member gets what the group carries: a grant
    let carries = crate::handlers::grants::group_permissions(id)?;
    crate::handlers::grants::require_may_grant(&state, &actor, &carries).await?;
    let address = req.address;
    let member = address.clone();
    with_kevy(move |c| Ok(groups::add_member(c, id, &member)?))?;
    record(&actor, "group.member_add", &id.to_string(), &address);
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_group_member(
    _: crate::session::confirm::Confirmed,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path((id, address)): Path<(i64, String)>,
) -> Result<StatusCode, StatusCode> {
    let member = address.clone();
    with_kevy(move |c| Ok(groups::remove_member(c, id, &member)?))?;
    record(&actor, "group.member_remove", &id.to_string(), &address);
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
    /// Initial membership, written to the member set. The UI creates
    /// groups empty and adds members through
    /// `POST /admin/email-groups/{id}/members`.
    #[serde(default)]
    pub members: Vec<String>,
}

/// The one writer of `email_group.create`: the MCP tool comes through here.
pub async fn create_email_group(
    _: crate::session::confirm::Confirmed,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Json(req): Json<CreateEmailGroupRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    crate::handlers::masked_email::refuse_if_masked(&req.address)?;
    let id = with_kevy(|c| next_id(c, EG_CTR))?;
    // members live only in the member set, never in the row
    let g = serde_json::json!({
        "id": id,
        "address": req.address,
        "domain": req.domain,
        "name": req.name,
        "description": req.description,
        "created_at": now_secs(),
    });
    let payload = serde_json::to_vec(&g).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let members = req.members;
    with_kevy(move |c| {
        c.hset(
            EG_KEY.as_bytes(),
            &[(id.to_string().as_bytes(), payload.as_slice())],
        )?;
        let refs: Vec<&[u8]> = members.iter().map(|m| m.as_bytes()).collect();
        if !refs.is_empty() {
            c.sadd(email_group_members_key(id).as_bytes(), &refs)?;
        }
        Ok(())
    })?;
    record(&actor, "email_group.create", &id.to_string(), &req.address);
    Ok(Json(g))
}

/// The one writer of `email_group.delete`: the MCP tool comes through here.
pub async fn delete_email_group(
    _: crate::session::confirm::Confirmed,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    with_kevy(move |c| {
        c.hdel(EG_KEY.as_bytes(), &[id.to_string().as_bytes()])?;
        c.del(&[email_group_members_key(id).as_bytes()])?;
        Ok(())
    })?;
    record(&actor, "email_group.delete", &id.to_string(), "");
    Ok(StatusCode::NO_CONTENT)
}
