//! The addresses an administrator grants an account to send as. An
//! account's own address, the aliases pointing at it and its email groups
//! need no grant; `/api/auth/me` shows the union.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};
use mailrs_core_api::method::admin as wire;

use crate::WebState;
use crate::handlers::admin::map_err;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::require_permission;

/// GET /api/admin/accounts/{address}/send-as
pub async fn get_account_send_as(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(address): Path<String>,
) -> Result<Json<wire::SendAsList>, StatusCode> {
    require_permission(&state, &actor, "admin.accounts").await?;
    state
        .core
        .get_send_as(&address)
        .await
        .map(Json)
        .map_err(map_err)
}

/// PUT /api/admin/accounts/{address}/send-as — replace the grants. 400 for
/// an entry that is not an address, 404 for an address that is not an
/// account.
pub async fn set_account_send_as(
    _: crate::session::confirm::Confirmed,
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(address): Path<String>,
    Json(req): Json<wire::SendAsList>,
) -> Result<StatusCode, StatusCode> {
    store_send_as(&state, &actor, &address, req).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Shared by the REST route and the MCP tool: permission, write, audit.
pub(crate) async fn store_send_as(
    state: &Arc<WebState>,
    actor: &str,
    address: &str,
    req: wire::SendAsList,
) -> Result<(), StatusCode> {
    require_permission(state, actor, "admin.accounts").await?;
    crate::handlers::grants::require_may_touch(state, actor, address).await?;
    // checked here too: the core client reports the core's 400 as a 500
    mailrs_core_sidestate::families::send_as::normalize_grants(&req.send_as)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    state
        .core
        .set_send_as(address, &req)
        .await
        .map_err(map_err)?;
    crate::handlers::audit::record(actor, "account.send_as", address, &req.send_as.join(","));
    Ok(())
}
