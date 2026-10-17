//! What an account may do: its own permissions plus those of every
//! permission group it belongs to. Every authorization check (webapi's
//! `require_admin` / `require_permission`, MCP's `require_admin`) asks this
//! route.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mailrs_core_sidestate::families::permission_groups::{self, Membership};

use crate::*;

/// `GET /v1/admin/accounts/{address}/effective-permissions`.
///
/// `is_super` comes only from the account's own blob; groups add
/// permission strings and group rows. An account with no blob of its own
/// is still answered when a group holds it.
pub(crate) async fn effective_permissions(
    State(state): State<Arc<FastcoreState>>,
    Path(address): Path<String>,
) -> Result<Json<adm::EffectivePermissionsResponse>, StatusCode> {
    let own = match state.mailbox.get_permissions_blob(&address) {
        Ok(Some(json)) => Some(
            serde_json::from_str::<adm::EffectivePermissionsResponse>(&json).map_err(|e| {
                tracing::warn!(error = %e, %address, "permissions blob does not decode");
                StatusCode::INTERNAL_SERVER_ERROR
            })?,
        ),
        Ok(None) => None,
        Err(e) => {
            tracing::warn!(error = %e, %address, "get_permissions_blob failed");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    let member = group_membership(&state, &address).await;
    if own.is_none() && member.groups.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }
    let mut perms = own.unwrap_or_else(|| adm::EffectivePermissionsResponse {
        address: address.clone(),
        permissions: Vec::new(),
        groups: Vec::new(),
        is_super: false,
        send_as: Vec::new(),
    });
    perms.permissions.extend(member.permissions);
    perms.permissions.sort();
    perms.permissions.dedup();
    for g in member.groups {
        if !perms.groups.iter().any(|have| have.id == g.id) {
            perms.groups.push(g);
        }
    }
    Ok(Json(perms))
}

/// Deliberate degradation: when the network kevy cannot be reached the
/// account keeps its own permissions and loses only what groups grant, so a
/// kevy-server outage cannot lock every admin out. Logged at warn.
async fn group_membership(state: &Arc<FastcoreState>, address: &str) -> Membership {
    let Some(url) = state.net_url.clone() else {
        return Membership::default();
    };
    let addr = address.to_string();
    let read = tokio::task::spawn_blocking(move || {
        let mut conn = kevy_client::Connection::connect(&url)?;
        permission_groups::membership(&mut conn, &addr)
    })
    .await;
    match read {
        Ok(Ok(m)) => m,
        Ok(Err(e)) => {
            tracing::warn!(error = %e, %address, "permission groups unreadable; own permissions only");
            Membership::default()
        }
        Err(e) => {
            tracing::warn!(error = %e, %address, "permission group read panicked; own permissions only");
            Membership::default()
        }
    }
}
