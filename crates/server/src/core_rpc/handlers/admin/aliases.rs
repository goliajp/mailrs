//! Aliases and domains.

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
    extract::{Path, State},
    http::StatusCode,
};

use mailrs_core_api::method::admin as wire;

use crate::core_rpc::CoreRpcState;

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

// Sieve read AND write are the shared network-kevy family now — see
// core-sidestate/families/groups_admin.rs. Nothing sieve-shaped belongs here.

// ── audit log ───────────────────────────────────────────────────────

// ── groups + permissions ────────────────────────────────────────────
