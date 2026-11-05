//! Per-user sender whitelist / blacklist CRUD (v2.4.1 roadmap
//! Phase 3.5, RFC 20260711 Phase B).
//!
//! Storage: `spam:{user}:whitelist` and `spam:{user}:blacklist`
//! kevy sets of lowercased email addresses. Read by the receiver
//! antispam pipeline (see `crates/receiver/src/spam_lists.rs`) and
//! written by:
//!
//!  - The mark-not-junk conversation action (auto-adds every
//!    thread sender to `whitelist`) — `handlers/conversations.rs`.
//!  - This module (explicit user management from the Settings UI).
//!
//!  - The block-sender conversation action — `handlers/block_sender.rs`.
//!
//! An address is on one list at most: adding to one takes it off the
//! other (`sender_lists::block` / `allow`), because delivery reads the
//! whitelist first and an address on both would never be blocked.

use std::sync::Arc;

use axum::Extension;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use mailrs_core_sidestate::families::sender_lists::{self, blacklist_key, whitelist_key};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

#[derive(serde::Deserialize)]
pub struct AddRequest {
    pub address: String,
}

fn list_set(user: &str, key: &str) -> Result<Vec<String>, StatusCode> {
    let key_owned = key.to_string();
    let _ = user;
    let members: Vec<Vec<u8>> = with_kevy(move |c| {
        c.smembers(key_owned.as_bytes())
            .map_err(std::io::Error::from)
    })
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut out: Vec<String> = members
        .into_iter()
        .filter_map(|b| String::from_utf8(b).ok())
        .collect();
    out.sort_unstable();
    Ok(out)
}

/// The address as both lists store it, or 400 when it is not one.
fn listed_address(address: &str) -> Result<String, StatusCode> {
    let addr = address.trim().to_lowercase();
    if addr.is_empty() || !addr.contains('@') {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(addr)
}

/// Onto the blacklist (`blocked`) or the whitelist, and off the other.
/// Answers whether it was on the other list.
///
/// The account's own address is never blocked (422): the reader's own
/// replies and notes would go to Junk with it.
pub(crate) fn put_on_list(user: &str, address: &str, blocked: bool) -> Result<bool, StatusCode> {
    let addr = listed_address(address)?;
    if blocked && addr == user.to_lowercase() {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let user = user.to_string();
    with_kevy(move |c| match blocked {
        true => sender_lists::block(c, &user, &addr),
        false => sender_lists::allow(c, &user, &addr),
    })
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub(crate) fn remove_from_set(key: &str, address: &str) -> Result<(), StatusCode> {
    let addr = address.to_lowercase();
    let key_owned = key.to_string();
    with_kevy(move |c| {
        c.srem(key_owned.as_bytes(), &[addr.as_bytes()])?;
        Ok(())
    })
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// GET /api/spam/whitelist — list the caller's whitelist entries.
pub async fn list_whitelist(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let entries = list_set(&user, &whitelist_key(&user))?;
    Ok(Json(serde_json::json!({ "entries": entries })))
}

/// POST /api/spam/whitelist { "address": "friend@example.com" }
pub async fn add_whitelist(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<AddRequest>,
) -> Result<StatusCode, StatusCode> {
    put_on_list(&user, &req.address, false)?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/spam/whitelist/{address}
pub async fn remove_whitelist(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(address): Path<String>,
) -> Result<StatusCode, StatusCode> {
    remove_from_set(&whitelist_key(&user), &address)?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/spam/blacklist
pub async fn list_blacklist(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let entries = list_set(&user, &blacklist_key(&user))?;
    Ok(Json(serde_json::json!({ "entries": entries })))
}

/// POST /api/spam/blacklist { "address": "spammer@evil.com" }
pub async fn add_blacklist(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<AddRequest>,
) -> Result<StatusCode, StatusCode> {
    put_on_list(&user, &req.address, true)?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/spam/blacklist/{address}
pub async fn remove_blacklist(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(address): Path<String>,
) -> Result<StatusCode, StatusCode> {
    remove_from_set(&blacklist_key(&user), &address)?;
    Ok(StatusCode::NO_CONTENT)
}
