//! Admin handlers — Phase 2.2 priority subset for webapi split unblock.
//!
//! Covers the auth hot-path:
//! - GET /v1/admin/api-keys/by-prefix/{prefix}     — every API-key authed req
//! - POST /v1/admin/api-keys/{id}/touch            — async update_last_used
//! - GET /v1/admin/accounts/{address}/effective-permissions  — every authed req
//! - GET /v1/admin/accounts/{address}/credentials  — SMTP/IMAP/POP3 AUTH
//! - GET /v1/admin/accounts                        — list (admin web UI)
//! - GET /v1/admin/accounts/{address}              — get one (admin web UI)
//!
//! Remaining 50+ admin endpoints land in subsequent loops.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::TimeZone;

use mailrs_core_api::method::admin as wire;

use crate::core_rpc::CoreRpcState;

// ── api keys ────────────────────────────────────────────────────────

/// GET /v1/admin/api-keys/by-prefix/{prefix}
///
/// Returns the API key row for AUTH verify. `key_hash` field is `#[serde(skip)]`
/// in `ApiKeyWire` — caller hashes the candidate full_key and compares
/// server-side. Returning the hash via wire would be a security regression.
///
/// Note: today this handler is incomplete — the wire type intentionally
/// elides `key_hash` for security, but server-side cement needs the hash to
/// argon2-verify. Until checklist 2.5 wraps internal auth, we expose `key_hash`
/// only when the caller is on the internal network (assumed via
/// `core-rpc` channel having `MAILRS_CORE_API_SECRET` bearer auth).
pub async fn get_api_key_by_prefix(
    State(state): State<Arc<CoreRpcState>>,
    Path(prefix): Path<String>,
) -> Result<Json<wire::ApiKeyWire>, StatusCode> {
    let row = crate::api_key_store::get_api_key_by_prefix(&state.pool, &prefix)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "api_key by prefix lookup failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(wire::ApiKeyWire {
        id: row.id,
        prefix: row.prefix,
        full_key: row.full_key,
        key_hash: row.key_hash,
        account_address: row.account_address,
        name: row.name,
        expires_at: row.expires_at.map(|d| d.timestamp()),
        last_used_at: row.last_used_at.map(|d| d.timestamp()),
        revoked_at: row.revoked_at.map(|d| d.timestamp()),
        created_at: row.created_at.timestamp(),
        app_id: row.app_id,
    }))
}

/// POST /v1/admin/api-keys/{id}/touch
///
/// Update `last_used_at = now()`. Fire-and-forget on caller side; we still
/// wait + log on error so the operator sees if writes are failing.
pub async fn touch_api_key(
    State(state): State<Arc<CoreRpcState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    crate::api_key_store::update_last_used(&state.pool, id).await;
    Ok(StatusCode::NO_CONTENT)
}

// ── effective permissions ───────────────────────────────────────────

/// GET /v1/admin/accounts/{address}/effective-permissions
///
/// Called per authed web request (`/api/auth/me` + middleware). Critical
/// hot path — webapi caches the result in kevy session.
pub async fn effective_permissions(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
) -> Result<Json<wire::EffectivePermissionsResponse>, StatusCode> {
    let perms = state
        .domain
        .load_account_permissions(&address)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, address = %address, "load perms failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(wire::EffectivePermissionsResponse {
        address: address.clone(),
        permissions: perms.permission_list(),
        // Group resolution is left for a separate RPC call to keep this
        // endpoint cheap; webapi can hit /accounts/{address}/groups when it
        // needs the group list (rarely; not part of the auth-me hot path).
        groups: Vec::new(),
        is_super: perms.is_super(),
        send_as: perms.send_as().to_vec(),
    }))
}

// ── accounts (read) ─────────────────────────────────────────────────

/// GET /v1/admin/accounts/{address}/credentials
///
/// SMTP/IMAP/POP3/MgSieve AUTH path. Returns Account public fields + the
/// argon2 password hash. Bearer-secret-only; never expose to webapi public.
pub async fn get_account_with_hash(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
) -> Result<Json<wire::AccountWithHashWire>, StatusCode> {
    let (account, password_hash) = state
        .domain
        .get_account_with_hash(&address)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, address = %address, "get_account_with_hash failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    let public = wire::AccountWire {
        address: account.address,
        domain: account.domain,
        display_name: account.display_name,
        active: account.active,
        created_at: account.created_at,
        quota_bytes: account.quota_bytes,
        recovery_email: if account.recovery_email.is_empty() {
            None
        } else {
            Some(account.recovery_email)
        },
    };
    Ok(Json(wire::AccountWithHashWire {
        public,
        password_hash: Some(password_hash),
    }))
}

/// GET /v1/admin/accounts
pub async fn list_accounts(
    State(state): State<Arc<CoreRpcState>>,
) -> Result<Json<wire::AccountListResponse>, StatusCode> {
    let rows = state.domain.list_accounts().await.map_err(|e| {
        tracing::warn!(error = %e, "list_accounts failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let items = rows
        .into_iter()
        .map(|a| wire::AccountWire {
            address: a.address,
            domain: a.domain,
            display_name: a.display_name,
            active: a.active,
            created_at: a.created_at,
            quota_bytes: a.quota_bytes,
            recovery_email: if a.recovery_email.is_empty() {
                None
            } else {
                Some(a.recovery_email)
            },
        })
        .collect();
    Ok(Json(wire::AccountListResponse { items }))
}

/// GET /v1/admin/accounts/{address}
///
/// Public account info (no hash). Caches via the same DomainStore kevy LRU.
pub async fn get_account(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
) -> Result<Json<wire::AccountWire>, StatusCode> {
    let (account, _) = state
        .domain
        .get_account_with_hash(&address)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, address = %address, "get_account failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(wire::AccountWire {
        address: account.address,
        domain: account.domain,
        display_name: account.display_name,
        active: account.active,
        created_at: account.created_at,
        quota_bytes: account.quota_bytes,
        recovery_email: if account.recovery_email.is_empty() {
            None
        } else {
            Some(account.recovery_email)
        },
    }))
}

// Silence unused imports under future loops:
#[allow(dead_code)]
fn _import_tz() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc.timestamp_opt(0, 0).unwrap()
}

// ── aliases ─────────────────────────────────────────────────────────

/// GET /v1/admin/aliases
pub async fn list_aliases(
    State(state): State<Arc<CoreRpcState>>,
) -> Result<Json<wire::AliasListResponse>, StatusCode> {
    let rows = state.domain.list_aliases().await.map_err(|e| {
        tracing::warn!(error = %e, "list_aliases failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let items = rows
        .into_iter()
        .map(|a| wire::AliasWire {
            id: a.id,
            source_address: a.source_address,
            target_address: a.target_address,
            domain: a.domain,
            alias_type: a.alias_type,
            active: a.active,
            created_at: a.created_at,
        })
        .collect();
    Ok(Json(wire::AliasListResponse { items }))
}

/// POST /v1/admin/aliases
pub async fn add_alias(
    State(state): State<Arc<CoreRpcState>>,
    Json(req): Json<wire::AddAliasRequest>,
) -> Result<Json<wire::AddAliasResponse>, StatusCode> {
    let now = chrono::Utc::now().timestamp();
    let id = state
        .domain
        .add_alias(
            &req.source_address,
            &req.target_address,
            &req.domain,
            &req.alias_type,
            now,
        )
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, source = %req.source_address, "add_alias failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(wire::AddAliasResponse { id }))
}

/// DELETE /v1/admin/aliases/{id}
pub async fn remove_alias(
    State(state): State<Arc<CoreRpcState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let removed = state.domain.remove_alias(id).await.map_err(|e| {
        tracing::warn!(error = %e, id, "remove_alias failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

// ── source-keyed alias API (v2 switchable-core boundary) ─────────────
// Both cores serve these identical routes: kevy is natively source-keyed,
// PG delegates to DomainStore::{upsert,remove}_alias_by_source. This is
// the backend-neutral alias surface webapi + mailrs-core-sync drive.

#[derive(serde::Deserialize)]
pub struct LocalAliasBody {
    pub source: String,
    pub target: String,
}

/// GET /v1/admin/aliases:local — `{ items: [{source, target}] }`.
pub async fn list_local_aliases(
    State(state): State<Arc<CoreRpcState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let rows = state.domain.list_aliases().await.map_err(|e| {
        tracing::warn!(error = %e, "list_local_aliases failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let items: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|a| serde_json::json!({"source": a.source_address, "target": a.target_address}))
        .collect();
    Ok(Json(serde_json::json!({ "items": items })))
}

/// POST /v1/admin/aliases:local — insert/replace one alias by source.
pub async fn upsert_local_alias(
    State(state): State<Arc<CoreRpcState>>,
    Json(body): Json<LocalAliasBody>,
) -> Result<StatusCode, StatusCode> {
    if body.source.is_empty() || body.target.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    state
        .domain
        .upsert_alias_by_source(&body.source, &body.target)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, source = %body.source, "upsert_local_alias failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /v1/admin/aliases:local/{source}
pub async fn delete_local_alias(
    State(state): State<Arc<CoreRpcState>>,
    Path(source): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .domain
        .remove_alias_by_source(&source)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, %source, "delete_local_alias failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── domains ─────────────────────────────────────────────────────────

/// GET /v1/admin/domains
pub async fn list_domains(
    State(state): State<Arc<CoreRpcState>>,
) -> Result<Json<wire::DomainListResponse>, StatusCode> {
    let rows = state.domain.list_domains().await.map_err(|e| {
        tracing::warn!(error = %e, "list_domains failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let items = rows
        .into_iter()
        .map(|d| wire::DomainWire {
            name: d.name,
            created_at: d.created_at,
        })
        .collect();
    Ok(Json(wire::DomainListResponse { items }))
}

/// POST /v1/admin/domains
pub async fn add_domain(
    State(state): State<Arc<CoreRpcState>>,
    Json(req): Json<wire::AddDomainRequest>,
) -> Result<StatusCode, StatusCode> {
    let now = chrono::Utc::now().timestamp();
    state.domain.add_domain(&req.name, now).await.map_err(|e| {
        tracing::warn!(error = %e, name = %req.name, "add_domain failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /v1/admin/domains/{name}
pub async fn remove_domain(
    State(state): State<Arc<CoreRpcState>>,
    Path(name): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let removed = state.domain.remove_domain(&name).await.map_err(|e| {
        tracing::warn!(error = %e, name = %name, "remove_domain failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

// ── sieve ───────────────────────────────────────────────────────────

/// GET /v1/admin/accounts/{address}/sieve
pub async fn get_sieve(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
) -> Result<Json<wire::SieveScriptResponse>, StatusCode> {
    let script = state.domain.get_sieve_script(&address).await.map_err(|e| {
        tracing::warn!(error = %e, address = %address, "get_sieve failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(wire::SieveScriptResponse { script }))
}

/// POST /v1/admin/accounts/{address}/sieve
pub async fn set_sieve(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
    Json(req): Json<wire::SetSieveRequest>,
) -> Result<StatusCode, StatusCode> {
    let now = chrono::Utc::now().timestamp();
    state
        .domain
        .set_sieve_script(&address, &req.script, now)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, address = %address, "set_sieve failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /v1/admin/accounts/{address}/sieve
pub async fn delete_sieve(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .domain
        .delete_sieve_script(&address)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, address = %address, "delete_sieve failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── audit log ───────────────────────────────────────────────────────

/// POST /v1/admin/audit-log
pub async fn log_audit(
    State(state): State<Arc<CoreRpcState>>,
    Json(req): Json<wire::LogAuditRequest>,
) -> StatusCode {
    state
        .domain
        .log_audit(&req.actor, &req.action, &req.target, &req.detail)
        .await;
    StatusCode::NO_CONTENT
}

// ── groups + permissions ────────────────────────────────────────────

fn group_info_to_wire(g: crate::permission::GroupInfo) -> wire::GroupWire {
    wire::GroupWire {
        id: g.id,
        name: g.name,
        domain: g.domain,
        description: g.description,
        is_builtin: g.is_builtin,
        created_at: g.created_at,
    }
}

/// GET /v1/admin/groups
pub async fn list_groups(
    State(state): State<Arc<CoreRpcState>>,
) -> Result<Json<wire::GroupListResponse>, StatusCode> {
    let rows = state.domain.list_groups(None).await.map_err(|e| {
        tracing::warn!(error = %e, "list_groups failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let items = rows.into_iter().map(group_info_to_wire).collect();
    Ok(Json(wire::GroupListResponse { items }))
}

/// GET /v1/admin/groups/{id}/permissions
pub async fn get_group_permissions(
    State(state): State<Arc<CoreRpcState>>,
    Path(id): Path<i64>,
) -> Result<Json<wire::GroupPermissionsResponse>, StatusCode> {
    let perms = state.domain.get_group_permissions(id).await.map_err(|e| {
        tracing::warn!(error = %e, id, "get_group_permissions failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(wire::GroupPermissionsResponse { permissions: perms }))
}

/// PUT /v1/admin/groups/{id}/permissions
pub async fn set_group_permissions(
    State(state): State<Arc<CoreRpcState>>,
    Path(id): Path<i64>,
    Json(req): Json<wire::SetGroupPermissionsRequest>,
) -> Result<StatusCode, StatusCode> {
    state
        .domain
        .set_group_permissions(id, &req.permissions)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, id, "set_group_permissions failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /v1/admin/groups/{id}/members
pub async fn list_group_members(
    State(state): State<Arc<CoreRpcState>>,
    Path(id): Path<i64>,
) -> Result<Json<wire::GroupMembersResponse>, StatusCode> {
    let members = state.domain.list_group_members(id).await.map_err(|e| {
        tracing::warn!(error = %e, id, "list_group_members failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(wire::GroupMembersResponse { members }))
}

/// POST /v1/admin/groups/{id}/members
pub async fn add_account_to_group(
    State(state): State<Arc<CoreRpcState>>,
    Path(id): Path<i64>,
    Json(req): Json<wire::AddGroupMemberRequest>,
) -> Result<StatusCode, StatusCode> {
    state
        .domain
        .add_account_to_group(&req.account_address, id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, id, "add_account_to_group failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /v1/admin/groups/{id}/members/{address}
pub async fn remove_account_from_group(
    State(state): State<Arc<CoreRpcState>>,
    Path((id, address)): Path<(i64, String)>,
) -> Result<StatusCode, StatusCode> {
    let removed = state
        .domain
        .remove_account_from_group(&address, id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, id, address = %address, "remove failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

/// GET /v1/admin/accounts/{address}/groups
pub async fn get_account_groups(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
) -> Result<Json<wire::GroupListResponse>, StatusCode> {
    let rows = state
        .domain
        .get_account_groups(&address)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, address = %address, "get_account_groups failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let items = rows.into_iter().map(group_info_to_wire).collect();
    Ok(Json(wire::GroupListResponse { items }))
}

/// GET /v1/admin/audit-log?limit=
pub async fn list_audit_log(
    State(state): State<Arc<CoreRpcState>>,
    Query(q): Query<wire::ListAuditQuery>,
) -> Result<Json<wire::AuditListResponse>, StatusCode> {
    let rows = state
        .domain
        .list_audit_log(q.limit as i64)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "list_audit_log failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let items = rows
        .into_iter()
        .map(|a| wire::AuditRowWire {
            id: a.id,
            timestamp: a.timestamp,
            actor: a.actor,
            action: a.action,
            target: a.target,
            detail: a.detail,
        })
        .collect();
    Ok(Json(wire::AuditListResponse { items }))
}
