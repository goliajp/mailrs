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
//! Blacklist entries are also written by the block-sender action
//! (deferred to Phase 3.7 alongside the sweep job) — the surface
//! here already handles the read + delete side for that flow.

use std::sync::Arc;

use axum::Extension;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

#[derive(serde::Deserialize)]
pub struct AddRequest {
    pub address: String,
}

fn whitelist_key(user: &str) -> String {
    format!("spam:{}:whitelist", user.to_lowercase())
}

fn blacklist_key(user: &str) -> String {
    format!("spam:{}:blacklist", user.to_lowercase())
}

fn list_set(user: &str, key: &str) -> Result<Vec<String>, StatusCode> {
    let key_owned = key.to_string();
    let _ = user;
    let members: Vec<Vec<u8>> = with_kevy(move |c| c.smembers(key_owned.as_bytes()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut out: Vec<String> = members
        .into_iter()
        .filter_map(|b| String::from_utf8(b).ok())
        .collect();
    out.sort_unstable();
    Ok(out)
}

fn add_to_set(key: &str, address: &str) -> Result<(), StatusCode> {
    let addr = address.trim().to_lowercase();
    if addr.is_empty() || !addr.contains('@') {
        return Err(StatusCode::BAD_REQUEST);
    }
    let key_owned = key.to_string();
    with_kevy(move |c| {
        c.sadd(key_owned.as_bytes(), &[addr.as_bytes()])?;
        Ok(())
    })
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn remove_from_set(key: &str, address: &str) -> Result<(), StatusCode> {
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
    add_to_set(&whitelist_key(&user), &req.address)?;
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
    add_to_set(&blacklist_key(&user), &req.address)?;
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
