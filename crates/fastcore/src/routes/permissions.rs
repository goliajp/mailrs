//! What an account may do: its own permissions plus those of every
//! permission group it belongs to. Every authorization check (webapi's
//! `require_admin` / `require_permission`, MCP's `require_admin`) asks this
//! route.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mailrs_core_sidestate::families::permission_groups;
use mailrs_core_sidestate::families::{email_groups, send_as};

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
    let member = {
        let (url, addr) = (state.net_url.clone(), address.clone());
        tokio::task::spawn_blocking(move || permission_groups::membership_at(url.as_deref(), &addr))
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };
    let mut perms = member
        .grant_to(own, &address)
        .ok_or(StatusCode::NOT_FOUND)?;
    let derived = derived_send_as(&state, &address).await;
    perms.send_as = send_as::union(&perms.send_as, derived.aliases, derived.groups);
    perms.masks = derived.masks;
    Ok(Json(perms))
}

struct DerivedSendAs {
    aliases: Vec<String>,
    groups: Vec<String>,
    masks: Vec<String>,
}

/// The aliases pointing at `address` and the email groups it belongs to.
/// Degrades like [`group_membership`]: when either store cannot be read
/// the account keeps the grants in its own blob and loses only these, so
/// an outage refuses some sends instead of every permission check.
async fn derived_send_as(state: &Arc<FastcoreState>, address: &str) -> DerivedSendAs {
    let st = state.clone();
    let addr = address.to_string();
    let read = tokio::task::spawn_blocking(move || read_derived(&st, &addr)).await;
    match read {
        Ok(Ok(d)) => d,
        Ok(Err(e)) => {
            tracing::warn!(error = %e, %address, "send-as aliases or groups unreadable; own grants only");
            DerivedSendAs::none()
        }
        Err(e) => {
            tracing::warn!(error = %e, %address, "send-as read panicked; own grants only");
            DerivedSendAs::none()
        }
    }
}

impl DerivedSendAs {
    fn none() -> Self {
        Self {
            aliases: Vec::new(),
            groups: Vec::new(),
            masks: Vec::new(),
        }
    }
}

fn read_derived(state: &FastcoreState, addr: &str) -> std::io::Result<DerivedSendAs> {
    let pairs = state.alias_store.list()?;
    let aliases =
        send_as::alias_sources_for(pairs.iter().map(|(s, t)| (s.as_str(), t.as_str())), addr);
    let (groups, masks) = match state.net_url.as_deref() {
        Some(url) => {
            let mut conn = kevy_client::Connection::connect(url)?;
            (
                email_groups::addresses_with_member(&mut conn, addr)?,
                mailrs_core_sidestate::families::masked_email::sendable(&mut conn, addr)?,
            )
        }
        None => (Vec::new(), Vec::new()),
    };
    Ok(DerivedSendAs {
        aliases,
        groups,
        masks,
    })
}

/// Whether `user` may put `from` on mail it sends, decided as the
/// effective-permissions route decides it, for callers off the runtime
/// (the spool drain). Degrades the same way: unreadable stores cost the
/// derived addresses, never grant one.
pub(crate) fn may_send_as_blocking(state: &FastcoreState, user: &str, from: &str) -> bool {
    let own = match state.mailbox.get_permissions_blob(user) {
        Ok(Some(json)) => serde_json::from_str::<adm::EffectivePermissionsResponse>(&json).ok(),
        Ok(None) => None,
        Err(e) => {
            tracing::warn!(error = %e, %user, "get_permissions_blob failed; own address only");
            None
        }
    };
    let mut perms = own.unwrap_or_else(|| adm::EffectivePermissionsResponse {
        address: user.to_string(),
        permissions: Vec::new(),
        groups: Vec::new(),
        is_super: false,
        send_as: Vec::new(),
        masks: Vec::new(),
    });
    let derived = match read_derived(state, user) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(error = %e, %user, "send-as aliases or groups unreadable; own grants only");
            DerivedSendAs::none()
        }
    };
    perms.send_as = send_as::union(&perms.send_as, derived.aliases, derived.groups);
    perms.masks = derived.masks;
    perms.may_send_as(from)
}

/// `GET /v1/admin/accounts/{address}/send-as` — the granted list only.
pub(crate) async fn get_send_as(
    State(state): State<Arc<FastcoreState>>,
    Path(address): Path<String>,
) -> Result<Json<adm::SendAsList>, StatusCode> {
    let blob = own_blob(&state, &address)?;
    Ok(Json(adm::SendAsList {
        send_as: blob.send_as,
    }))
}

/// `PUT /v1/admin/accounts/{address}/send-as` — replace the granted list.
/// 404 for an address that is not an account; 400 names an entry that is
/// not an address.
pub(crate) async fn set_send_as(
    State(state): State<Arc<FastcoreState>>,
    Path(address): Path<String>,
    Json(req): Json<adm::SendAsList>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let grants = match send_as::normalize_grants(&req.send_as) {
        Ok(g) => g,
        Err(bad) => {
            return (StatusCode::BAD_REQUEST, format!("not an address: {bad}")).into_response();
        }
    };
    match state.mailbox.get_account_blob(&address) {
        Ok(Some(_)) => {}
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::warn!(error = %e, %address, "get_account_blob failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }
    let mut blob = match own_blob(&state, &address) {
        Ok(b) => b,
        Err(s) => return s.into_response(),
    };
    blob.send_as = grants;
    let json = match serde_json::to_string(&blob) {
        Ok(j) => j,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    match state.mailbox.upsert_permissions(&address, &json) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::warn!(error = %e, %address, "upsert_permissions failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// The account's own permissions blob, or the empty one an account
/// created without a blob would have had.
fn own_blob(
    state: &Arc<FastcoreState>,
    address: &str,
) -> Result<adm::EffectivePermissionsResponse, StatusCode> {
    match state.mailbox.get_permissions_blob(address) {
        Ok(Some(json)) => serde_json::from_str(&json).map_err(|e| {
            tracing::warn!(error = %e, %address, "permissions blob does not decode");
            StatusCode::INTERNAL_SERVER_ERROR
        }),
        Ok(None) => Ok(adm::EffectivePermissionsResponse {
            address: address.to_string(),
            permissions: Vec::new(),
            groups: Vec::new(),
            is_super: false,
            send_as: Vec::new(),
            masks: Vec::new(),
        }),
        Err(e) => {
            tracing::warn!(error = %e, %address, "get_permissions_blob failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
