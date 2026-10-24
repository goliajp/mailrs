//! Accounts, aliases, domains and email-group members — the directory
//! the admin UI edits. Accounts are in `accounts.rs`.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};
use mailrs_core_api::method::admin as wire;

use crate::WebState;
use crate::handlers::admin::*;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

mod accounts;
mod send_as;
pub use accounts::*;
pub use send_as::*;

/// One-shot boot-time mirror of network-kevy alias entries into the
/// fastcore-embedded alias table. Reads every `admin:aliases` hash row,
/// deserializes the `AliasWire`, and calls `fastcore.upsert_local_alias`
/// for each `source → target` pair. Idempotent — safe to run every boot.
/// Returns the count of aliases successfully mirrored.
pub async fn sync_aliases_to_fastcore(state: &Arc<WebState>) -> usize {
    let vals = match with_kevy(|c| hgetall_values(c, ALIAS_KEY)) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    let mut synced = 0usize;
    for v in vals {
        let Ok(alias) = serde_json::from_slice::<wire::AliasWire>(&v) else {
            continue;
        };
        if !alias.active {
            continue;
        }
        if state
            .core
            .upsert_local_alias(&alias.source_address, &alias.target_address)
            .await
            .is_ok()
        {
            synced += 1;
        }
    }
    synced
}

/// One alias as the store holds it: a source address and its target.
/// The store keeps no type, creation time or active flag, so none is
/// reported. `id` is the source address, the store's own key.
#[derive(Debug, serde::Serialize)]
pub struct AliasRow {
    pub id: String,
    pub source_address: String,
    pub target_address: String,
    pub domain: String,
}

/// The `(source, target)` pairs fastcore's alias store holds.
async fn stored_aliases(state: &WebState) -> Result<Vec<(String, String)>, StatusCode> {
    let raw = state
        .core
        .list_local_aliases()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let items = raw.get("items").and_then(|v| v.as_array());
    Ok(items
        .into_iter()
        .flatten()
        .filter_map(|it| {
            let source = it.get("source")?.as_str()?.to_string();
            let target = it.get("target")?.as_str()?.to_string();
            Some((source, target))
        })
        .collect())
}

/// Every alias as the admin list reports it. Shared by REST and MCP.
pub(crate) async fn alias_rows(state: &WebState) -> Result<Vec<AliasRow>, StatusCode> {
    Ok(stored_aliases(state)
        .await?
        .into_iter()
        .map(|(source, target)| AliasRow {
            id: source.clone(),
            domain: source.split_once('@').map(|(_, d)| d).unwrap_or("").into(),
            source_address: source,
            target_address: target,
        })
        .collect())
}

/// Write `source → target` to fastcore's alias store. Shared by REST and MCP.
pub(crate) async fn store_alias(
    state: &WebState,
    source: &str,
    target: &str,
) -> Result<(), StatusCode> {
    state
        .core
        .upsert_local_alias(source, target)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    // a new source domain is added to the domain index, which the admin
    // dropdowns read; the alias is already written, so a failure only warns
    if let Some((_, domain)) = source.split_once('@')
        && let Err(e) = state.core.add_domain(domain).await
    {
        tracing::warn!(err = %e, %domain, "add_domain self-heal from add_alias failed");
    }
    Ok(())
}

/// Delete the alias whose source is `source`; 404 when the store holds
/// none. Shared by REST and MCP.
pub(crate) async fn delete_alias(state: &WebState, source: &str) -> Result<(), StatusCode> {
    if !stored_aliases(state)
        .await?
        .iter()
        .any(|(s, _)| s == source)
    {
        return Err(StatusCode::NOT_FOUND);
    }
    state
        .core
        .delete_local_alias(source)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// GET /api/admin/aliases — read from fastcore's alias store, the one
/// inbound delivery resolves against.
pub async fn list_aliases(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let items = alias_rows(&state).await?;
    Ok(Json(serde_json::json!({ "items": items })))
}

/// What creating an alias needs. The store holds only the pair; the
/// domain comes from the source address.
#[derive(Debug, serde::Deserialize)]
pub struct AddAliasRequest {
    pub source_address: String,
    pub target_address: String,
}

/// POST /api/admin/aliases — writes the `source → target` pair to
/// fastcore's alias store.
pub async fn add_alias(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Json(req): Json<AddAliasRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    store_alias(&state, &req.source_address, &req.target_address).await?;
    super::audit::record(
        &actor,
        "alias.create",
        &req.source_address,
        &format!("-> {}", req.target_address),
    );
    Ok(Json(serde_json::json!({ "id": req.source_address })))
}

/// DELETE /api/admin/aliases/{id} — `id` is the source address the list
/// reports. 404 when the store holds no such alias.
pub async fn remove_alias(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(source): Path<String>,
) -> Result<StatusCode, StatusCode> {
    delete_alias(&state, &source).await?;
    super::audit::record(&actor, "alias.delete", &source, "");
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/admin/domains
///
/// v2.2-fix (2026-07-09): read through fastcore RPC (embedded kevy
/// `mailrs:domains:index` set + per-name entries), same reason the
/// alias handler moved above — the legacy `admin:domains` network-kevy
/// hash was emptied at the fastcore split and returned `[]`, breaking
/// the alias-create form's domain dropdown and every downstream
/// domain-gated UI.
pub async fn list_domains(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<wire::DomainListResponse>, StatusCode> {
    state.core.list_domains().await.map(Json).map_err(map_err)
}

#[derive(Debug, serde::Deserialize)]
pub struct AddDomainBody {
    pub name: String,
}

/// POST /api/admin/domains — writes through fastcore RPC.
pub async fn add_domain(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Json(req): Json<AddDomainBody>,
) -> Result<StatusCode, StatusCode> {
    state.core.add_domain(&req.name).await.map_err(map_err)?;
    super::audit::record(&actor, "domain.create", &req.name, "");
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/admin/domains/{name} — deletes through fastcore RPC.
pub async fn remove_domain(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Result<StatusCode, StatusCode> {
    state.core.remove_domain(&name).await.map_err(map_err)?;
    super::audit::record(&actor, "domain.delete", &name, "");
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_email_group_members(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let key = mailrs_core_sidestate::families::email_groups::members_key(&id);
    let members = with_kevy(move |c| c.smembers(key.as_bytes()).map_err(std::io::Error::from))?;
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
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(id): Path<String>,
    Json(req): Json<AddMemberRequest>,
) -> Result<StatusCode, StatusCode> {
    let key = mailrs_core_sidestate::families::email_groups::members_key(&id);
    let addr = req.address;
    let addr_c = addr.clone();
    with_kevy(move |c| {
        c.sadd(key.as_bytes(), &[addr_c.as_bytes()])?;
        Ok(())
    })?;
    super::audit::record(&actor, "email_group.member_add", &id, &addr);
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_email_group_member(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path((id, address)): Path<(String, String)>,
) -> Result<StatusCode, StatusCode> {
    let key = mailrs_core_sidestate::families::email_groups::members_key(&id);
    let address_c = address.clone();
    with_kevy(move |c| {
        c.srem(key.as_bytes(), &[address_c.as_bytes()])?;
        Ok(())
    })?;
    super::audit::record(&actor, "email_group.member_remove", &id, &address);
    Ok(StatusCode::NO_CONTENT)
}
