//! An account's local forwards: other accounts here that get a copy of
//! everything delivered to it, written straight into their mailboxes by
//! the spool drain (`families::local_forward`). Only accounts can be
//! targets; mail for anywhere else goes through an alias or sieve.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};
use mailrs_core_api::error::CoreApiError;
use mailrs_core_sidestate::families::local_forward;
use serde::{Deserialize, Serialize};

use crate::WebState;
use crate::handlers::admin::map_err;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::{require_permission, with_kevy};

/// Body of `GET` and `PUT /api/admin/accounts/{address}/forwards`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalForwards {
    pub forward_to: Vec<String>,
}

/// GET /api/admin/accounts/{address}/forwards
pub async fn get_account_forwards(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(address): Path<String>,
) -> Result<Json<LocalForwards>, StatusCode> {
    load_forwards(&state, &actor, &address).await.map(Json)
}

/// PUT /api/admin/accounts/{address}/forwards — replace the targets. 404
/// when `address` is not an account; 400 when a target is not another
/// account here.
pub async fn set_account_forwards(
    _: crate::session::confirm::Confirmed,
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(address): Path<String>,
    Json(req): Json<LocalForwards>,
) -> Result<StatusCode, StatusCode> {
    store_forwards(&state, &actor, &address, req).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Shared by the REST route and the MCP tool.
pub(crate) async fn load_forwards(
    state: &Arc<WebState>,
    actor: &str,
    address: &str,
) -> Result<LocalForwards, StatusCode> {
    require_permission(state, actor, "admin.accounts").await?;
    let addr = address.to_string();
    let forward_to = with_kevy(move |c| local_forward::targets(c, &addr))?;
    Ok(LocalForwards { forward_to })
}

/// Shared by the REST route and the MCP tool: permission, checks, write,
/// audit.
pub(crate) async fn store_forwards(
    state: &Arc<WebState>,
    actor: &str,
    address: &str,
    req: LocalForwards,
) -> Result<(), StatusCode> {
    require_permission(state, actor, "admin.accounts").await?;
    crate::handlers::grants::require_may_touch(state, actor, address).await?;
    state
        .core
        .get_account_with_hash(address)
        .await
        .map_err(map_err)?;
    let mut targets: Vec<String> = Vec::with_capacity(req.forward_to.len());
    for raw in &req.forward_to {
        let target = raw.trim().to_ascii_lowercase();
        if target.is_empty() || target.eq_ignore_ascii_case(address.trim()) {
            return Err(StatusCode::BAD_REQUEST);
        }
        match state.core.get_account_with_hash(&target).await {
            Ok(_) => {}
            Err(CoreApiError::NotFound(_)) => return Err(StatusCode::BAD_REQUEST),
            Err(e) => return Err(map_err(e)),
        }
        if !targets.contains(&target) {
            targets.push(target);
        }
    }
    let addr = address.to_string();
    let to = targets.clone();
    with_kevy(move |c| local_forward::set_targets(c, &addr, &to))?;
    crate::handlers::audit::record(actor, "account.forwards", address, &targets.join(","));
    Ok(())
}
