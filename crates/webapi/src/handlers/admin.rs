//! `/api/admin/*` REST handlers.
//!
//! Phase 3 — handlers delegate to core RPC. Permission check (require
//! `admin.*`) happens server-side via core's effective_permissions; webapi
//! just relays the response. For the admin UI happy path we lock the
//! permission filter to the same shape the monolith uses
//! (effectively: any user with `admin.<resource>` perm), but here we
//! defer to core to enforce.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Query, State},
    http::StatusCode,
};

use mailrs_core_api::method::admin as wire;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

fn map_err(e: mailrs_core_api::error::CoreApiError) -> StatusCode {
    StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

/// GET /api/admin/accounts
pub async fn list_accounts(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<wire::AccountListResponse>, StatusCode> {
    state
        .core_client
        .list_accounts()
        .await
        .map(Json)
        .map_err(map_err)
}

/// GET /api/admin/aliases
pub async fn list_aliases(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<wire::AliasListResponse>, StatusCode> {
    state
        .core_client
        .list_aliases()
        .await
        .map(Json)
        .map_err(map_err)
}

/// GET /api/admin/domains
pub async fn list_domains(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<wire::DomainListResponse>, StatusCode> {
    state
        .core_client
        .list_domains()
        .await
        .map(Json)
        .map_err(map_err)
}

#[derive(Debug, serde::Deserialize)]
pub struct AuditQuery {
    #[serde(default = "default_audit_limit")]
    pub limit: u32,
}

fn default_audit_limit() -> u32 {
    100
}

/// POST /api/admin/accounts
pub async fn add_account(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Json(req): Json<wire::AddAccountRequest>,
) -> Result<StatusCode, StatusCode> {
    state
        .core_client
        .add_account(&req)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// DELETE /api/admin/accounts/{address}
pub async fn remove_account(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    axum::extract::Path(address): axum::extract::Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core_client
        .remove_account(&address)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/admin/aliases
pub async fn add_alias(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Json(req): Json<wire::AddAliasRequest>,
) -> Result<Json<wire::AddAliasResponse>, StatusCode> {
    state
        .core_client
        .add_alias(&req)
        .await
        .map(Json)
        .map_err(map_err)
}

/// DELETE /api/admin/aliases/{id}
pub async fn remove_alias(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Result<StatusCode, StatusCode> {
    state
        .core_client
        .remove_alias(id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

#[derive(Debug, serde::Deserialize)]
pub struct AddDomainBody {
    pub name: String,
}

/// POST /api/admin/domains
pub async fn add_domain(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Json(req): Json<AddDomainBody>,
) -> Result<StatusCode, StatusCode> {
    state
        .core_client
        .add_domain(&req.name)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// DELETE /api/admin/domains/{name}
pub async fn remove_domain(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core_client
        .remove_domain(&name)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// GET /api/admin/audit-log
pub async fn list_audit_log(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Query(q): Query<AuditQuery>,
) -> Result<Json<wire::AuditListResponse>, StatusCode> {
    state
        .core_client
        .list_audit_log(q.limit)
        .await
        .map(Json)
        .map_err(map_err)
}
