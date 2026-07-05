//! Groups / permissions / api-keys / sieve — admin side-state served from
//! the network kevy, the same keys webapi + pg-core use:
//!   `admin:groups`                    hash id → loose JSON group
//!   `admin:groups:{id}:permissions`   set of permission strings
//!   `admin:groups:{id}:members`       set of member addresses
//!   `admin:apikey:by-prefix:{prefix}` JSON ApiKeyWire (auth lookup)
//!   `admin:apikey:{id}:last_used`     epoch of last use
//!   `sieve:{address}`                 raw sieve script

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use mailrs_core_api::method::admin::{
    ApiKeyWire, GroupListResponse, GroupMembersResponse, GroupPermissionsResponse, GroupWire,
    SieveScriptResponse,
};

use crate::NetKevy;

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Parse a loose kevy group JSON (`{id,name,description,created_at}`, no
/// domain/is_builtin) into the full GroupWire.
fn parse_group(v: &[u8]) -> Option<GroupWire> {
    let j: serde_json::Value = serde_json::from_slice(v).ok()?;
    Some(GroupWire {
        id: j.get("id")?.as_i64()?,
        name: j.get("name").and_then(|x| x.as_str()).unwrap_or("").into(),
        domain: j.get("domain").and_then(|x| x.as_str()).map(String::from),
        description: j
            .get("description")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .into(),
        is_builtin: j
            .get("is_builtin")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        created_at: j.get("created_at").and_then(|x| x.as_i64()).unwrap_or(0),
    })
}

fn all_groups(conn: &mut kevy_client::Connection) -> Vec<GroupWire> {
    conn.hgetall(b"admin:groups")
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .filter_map(|(i, v)| if i % 2 == 1 { Some(v) } else { None })
        .filter_map(|v| parse_group(&v))
        .collect()
}

pub async fn list_groups<S: NetKevy>(State(state): State<Arc<S>>) -> Json<GroupListResponse> {
    let Some(mut conn) = state.net_conn() else {
        return Json(GroupListResponse { items: Vec::new() });
    };
    let mut items = all_groups(&mut conn);
    items.sort_by_key(|g| g.id);
    Json(GroupListResponse { items })
}

pub async fn get_group_permissions<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path(id): Path<i64>,
) -> Json<GroupPermissionsResponse> {
    let Some(mut conn) = state.net_conn() else {
        return Json(GroupPermissionsResponse {
            permissions: Vec::new(),
        });
    };
    let permissions = conn
        .smembers(format!("admin:groups:{id}:permissions").as_bytes())
        .unwrap_or_default()
        .into_iter()
        .map(|v| String::from_utf8_lossy(&v).into_owned())
        .collect();
    Json(GroupPermissionsResponse { permissions })
}

pub async fn list_group_members<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path(id): Path<i64>,
) -> Json<GroupMembersResponse> {
    let Some(mut conn) = state.net_conn() else {
        return Json(GroupMembersResponse {
            members: Vec::new(),
        });
    };
    let members = conn
        .smembers(format!("admin:groups:{id}:members").as_bytes())
        .unwrap_or_default()
        .into_iter()
        .map(|v| String::from_utf8_lossy(&v).into_owned())
        .collect();
    Json(GroupMembersResponse { members })
}

pub async fn get_account_groups<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path(address): Path<String>,
) -> Json<GroupListResponse> {
    let Some(mut conn) = state.net_conn() else {
        return Json(GroupListResponse { items: Vec::new() });
    };
    let groups = all_groups(&mut conn);
    let mut items = Vec::new();
    for g in groups {
        let members = conn
            .smembers(format!("admin:groups:{}:members", g.id).as_bytes())
            .unwrap_or_default();
        if members
            .iter()
            .any(|m| String::from_utf8_lossy(m) == address)
        {
            items.push(g);
        }
    }
    Json(GroupListResponse { items })
}

pub async fn remove_account_from_group<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path((id, address)): Path<(i64, String)>,
) -> StatusCode {
    let Some(mut conn) = state.net_conn() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    match conn.srem(
        format!("admin:groups:{id}:members").as_bytes(),
        &[address.as_bytes()],
    ) {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

// ── api keys ────────────────────────────────────────────────────────

pub async fn get_api_key_by_prefix<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path(prefix): Path<String>,
) -> Result<Json<ApiKeyWire>, StatusCode> {
    let mut conn = state.net_conn().ok_or(StatusCode::NOT_FOUND)?;
    let raw = conn
        .get(format!("admin:apikey:by-prefix:{prefix}").as_bytes())
        .ok()
        .flatten()
        .ok_or(StatusCode::NOT_FOUND)?;
    let key: ApiKeyWire = serde_json::from_slice(&raw).map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(Json(key))
}

pub async fn touch_api_key<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path(id): Path<i64>,
) -> StatusCode {
    let Some(mut conn) = state.net_conn() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let _ = conn.set(
        format!("admin:apikey:{id}:last_used").as_bytes(),
        now_secs().to_string().as_bytes(),
    );
    StatusCode::NO_CONTENT
}

// ── sieve ───────────────────────────────────────────────────────────

pub async fn get_sieve<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path(address): Path<String>,
) -> Json<SieveScriptResponse> {
    let script = state.net_conn().and_then(|mut conn| {
        conn.get(format!("sieve:{address}").as_bytes())
            .ok()
            .flatten()
            .map(|v| String::from_utf8_lossy(&v).into_owned())
    });
    Json(SieveScriptResponse { script })
}
