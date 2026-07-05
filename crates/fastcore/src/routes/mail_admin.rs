//! Account / alias / domain contract routes backed by the switchable
//! mail store (fastcore's embedded kevy). These mirror the pg-core admin
//! handlers so both cores serve the identical `/v1/admin/*` surface.
//!
//! kevy has no alias/domain row ids, so alias ids are a STABLE hash of the
//! source address — `list_aliases` and `remove_alias/{id}` agree on it.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use mailrs_core_api::method::admin::{
    AccountWire, AccountWithHashWire, AddAliasRequest, AddAliasResponse, AddDomainRequest,
    AliasListResponse, AliasWire, DomainListResponse, DomainWire,
};

use crate::FastcoreState;

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Deterministic i64 id for a source address (kevy stores no alias id).
/// `DefaultHasher::new()` uses fixed keys, so this is stable across runs.
fn alias_id(source: &str) -> i64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut h);
    (h.finish() >> 1) as i64
}

fn domain_of(addr: &str) -> String {
    addr.rsplit_once('@').map(|(_, d)| d).unwrap_or("").into()
}

// ── accounts ────────────────────────────────────────────────────────

pub async fn get_account(
    State(state): State<Arc<FastcoreState>>,
    Path(address): Path<String>,
) -> Result<Json<AccountWire>, StatusCode> {
    match state.mailbox.get_account_blob(&address) {
        Ok(Some(json)) => {
            let with_hash: AccountWithHashWire =
                serde_json::from_str(&json).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            Ok(Json(with_hash.public))
        }
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

// ── aliases ─────────────────────────────────────────────────────────

pub async fn list_aliases(
    State(state): State<Arc<FastcoreState>>,
) -> Result<Json<AliasListResponse>, StatusCode> {
    let pairs = state
        .mailbox
        .list_aliases()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut items: Vec<AliasWire> = pairs
        .into_iter()
        .map(|(source, target)| AliasWire {
            id: alias_id(&source),
            domain: domain_of(&source),
            source_address: source,
            target_address: target,
            alias_type: "alias".into(),
            active: true,
            created_at: 0,
        })
        .collect();
    items.sort_by(|a, b| a.source_address.cmp(&b.source_address));
    Ok(Json(AliasListResponse { items }))
}

pub async fn add_alias(
    State(state): State<Arc<FastcoreState>>,
    Json(req): Json<AddAliasRequest>,
) -> Result<Json<AddAliasResponse>, StatusCode> {
    state
        .mailbox
        .upsert_alias(&req.source_address, &req.target_address)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(AddAliasResponse {
        id: alias_id(&req.source_address),
    }))
}

pub async fn remove_alias(
    State(state): State<Arc<FastcoreState>>,
    Path(id): Path<i64>,
) -> StatusCode {
    let pairs = match state.mailbox.list_aliases() {
        Ok(p) => p,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR,
    };
    let Some((source, _)) = pairs.into_iter().find(|(s, _)| alias_id(s) == id) else {
        return StatusCode::NOT_FOUND;
    };
    match state.mailbox.delete_alias(&source) {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

// ── domains ─────────────────────────────────────────────────────────

pub async fn list_domains(
    State(state): State<Arc<FastcoreState>>,
) -> Result<Json<DomainListResponse>, StatusCode> {
    let rows = state
        .mailbox
        .list_domains()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let items = rows
        .into_iter()
        .map(|(name, created_at)| DomainWire { name, created_at })
        .collect();
    Ok(Json(DomainListResponse { items }))
}

pub async fn add_domain(
    State(state): State<Arc<FastcoreState>>,
    Json(req): Json<AddDomainRequest>,
) -> Result<StatusCode, StatusCode> {
    state
        .mailbox
        .upsert_domain(&req.name, now_secs())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_domain(
    State(state): State<Arc<FastcoreState>>,
    Path(name): Path<String>,
) -> StatusCode {
    match state.mailbox.delete_domain(&name) {
        Ok(true) => StatusCode::NO_CONTENT,
        Ok(false) => StatusCode::NOT_FOUND,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
