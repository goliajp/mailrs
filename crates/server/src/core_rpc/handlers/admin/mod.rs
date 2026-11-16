//! Admin handlers: accounts, their permissions and credentials, local
//! aliases and domains.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::TimeZone;

use mailrs_core_api::method::admin as wire;

use crate::core_rpc::CoreRpcState;

// ── api keys ────────────────────────────────────────────────────────

// ── effective permissions ───────────────────────────────────────────

pub(crate) mod account_purge;
mod aliases;
pub(crate) mod group_purge;
mod ownerless;
mod quota;
mod rejudge;
mod sent_to;

pub(crate) use aliases::*;
pub(crate) use ownerless::*;
pub(crate) use quota::{quota_limits_route, usage_drift_apply_route, usage_drift_route};
pub(crate) use rejudge::*;
pub(crate) use sent_to::backfill_sent_to_route;

/// GET /v1/admin/accounts/{address}/effective-permissions
///
/// Called per authed web request (`/api/auth/me` + middleware). Critical
/// hot path — webapi caches the result in kevy session.
pub async fn effective_permissions(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
) -> Result<Json<wire::EffectivePermissionsResponse>, StatusCode> {
    effective_permissions_of(&state.domain, state.net_url.as_deref(), &address)
        .await
        .map(Json)
}

/// What `address` may do, as the route answers it; the spool drain asks
/// it too, for who a vacation reply may be sent as.
pub(crate) async fn effective_permissions_of(
    domain: &crate::domain_store::DomainStore,
    net_url: Option<&str>,
    address: &str,
) -> Result<wire::EffectivePermissionsResponse, StatusCode> {
    let is_account = domain
        .get_account_with_hash(address)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, address = %address, "load account failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .is_some();
    let member = {
        let (url, addr) = (net_url.map(str::to_string), address.to_string());
        tokio::task::spawn_blocking(move || {
            mailrs_core_sidestate::families::permission_groups::membership_at(url.as_deref(), &addr)
        })
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };
    let own = match is_account {
        true => {
            let perms = domain
                .load_account_permissions(address)
                .await
                .map_err(|e| {
                    tracing::warn!(error = %e, address = %address, "load perms failed");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            Some(wire::EffectivePermissionsResponse {
                address: address.to_string(),
                permissions: perms.permission_list(),
                groups: Vec::new(),
                is_super: perms.is_super(),
                send_as: Vec::new(),
                masks: Vec::new(),
            })
        }
        false => None,
    };
    let mut perms = member.grant_to(own, address).ok_or(StatusCode::NOT_FOUND)?;
    // degrades as the kevy core does: an unreadable alias or group store
    // costs the derived addresses, not the permission check
    perms.send_as = match domain.send_as(address, net_url).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, address = %address, "send-as unreadable; own grants only");
            domain.send_as_grants(address).await.unwrap_or_default()
        }
    };
    perms.masks = masks_of(net_url.map(str::to_string), address).await;
    Ok(perms)
}

/// The account's masks that take mail, from the network kevy both cores
/// share. Degrades as the derived send-as does: unreadable costs the masks,
/// not the permission check.
async fn masks_of(net_url: Option<String>, address: &str) -> Vec<String> {
    let Some(url) = net_url else {
        return Vec::new();
    };
    let addr = address.to_string();
    let read = tokio::task::spawn_blocking(move || {
        let mut conn = kevy_client::Connection::connect(&url).map_err(std::io::Error::other)?;
        mailrs_core_sidestate::families::masked_email::sendable(&mut conn, &addr)
    })
    .await;
    match read {
        Ok(Ok(masks)) => masks,
        Ok(Err(e)) => {
            tracing::warn!(error = %e, %address, "masks unreadable; own grants only");
            Vec::new()
        }
        Err(e) => {
            tracing::warn!(error = %e, %address, "masks read panicked; own grants only");
            Vec::new()
        }
    }
}

/// GET /v1/admin/accounts/{address}/send-as
pub async fn get_send_as(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
) -> Result<Json<wire::SendAsList>, StatusCode> {
    let send_as = state
        .domain
        .send_as_grants(&address)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(wire::SendAsList { send_as }))
}

/// PUT /v1/admin/accounts/{address}/send-as
pub async fn set_send_as(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
    Json(req): Json<wire::SendAsList>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let grants = match mailrs_core_sidestate::families::send_as::normalize_grants(&req.send_as) {
        Ok(g) => g,
        Err(bad) => {
            return (StatusCode::BAD_REQUEST, format!("not an address: {bad}")).into_response();
        }
    };
    match state.domain.get_account_with_hash(&address).await {
        Ok(Some(_)) => {}
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
    match state.domain.set_send_as_grants(&address, &grants).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

// ── accounts (read) ─────────────────────────────────────────────────

// Silence unused imports under future loops:
#[allow(dead_code)]
fn _import_tz() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc.timestamp_opt(0, 0).unwrap()
}

// ── account writes (v2 dual-mode parity with fastcore) ──────────────
// The monolith did account CRUD in its web layer; in the split the PG
// core must serve these so webapi (a separate process on one core
// client) can create/update accounts regardless of backend.

/// POST /v1/users/{user}/messages/{uid}/flags — set flags verbatim on
/// the message in the user's INBOX (parity with fastcore's per-user
/// uid index route).
pub async fn set_message_flags(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, uid)): Path<(String, u32)>,
    Json(req): Json<wire::SetMessageFlagsRequest>,
) -> Result<StatusCode, StatusCode> {
    use mailrs_mailbox::MailboxStore;
    let mb = state
        .mailbox
        .get_mailbox(&user, "INBOX")
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    state
        .mailbox
        .set_flags(mb.id, uid, req.flags)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, uid, "set_message_flags failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

// ── aliases ─────────────────────────────────────────────────────────
