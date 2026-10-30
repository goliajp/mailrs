//! An administrator setting someone's password, over REST and MCP alike.
//!
//! A routine reset signs the account's sessions out and leaves its agent
//! keys: they are separate credentials, and services run on them. A reset
//! after a compromise revokes every key too, since whoever had the account
//! may have made one. The administrator names the keys they were shown,
//! and the reset goes ahead only while that is still the whole set: the
//! list is there so they know which services stop, not to choose.

use std::sync::Arc;

use axum::http::StatusCode;
use kevy_client::Connection;
use serde::Serialize;

use crate::WebState;
use crate::handlers::admin::map_err;
use crate::handlers::kevy_util::with_kevy;

/// The day agent keys began to record their last use. A key with no
/// recorded time was not used since then, or was revoked before it.
pub const LAST_USE_RECORDED_SINCE: &str = "2026-10-31";

/// One agent key as the administrator sees it before a compromise reset.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct KeyRow {
    pub id: i64,
    pub name: String,
    pub prefix: String,
    pub scopes: Vec<String>,
    /// Unix seconds; `None` for a record that never carried one.
    pub created_at: Option<i64>,
    /// Unix seconds, to the hour; `None` when nothing was recorded.
    pub last_used_at: Option<i64>,
}

/// `address`'s agent keys, by id.
pub(crate) fn key_rows(c: &mut Connection, address: &str) -> std::io::Result<Vec<KeyRow>> {
    let used = crate::agent_key_usage::of_owner(c, address)?;
    let records = crate::handlers::admin::hgetall_values(c, &format!("agent:keys:{address}"))?;
    let mut rows: Vec<KeyRow> = records
        .iter()
        .filter_map(|raw| serde_json::from_slice::<serde_json::Value>(raw).ok())
        .filter_map(|v| {
            let id = v["id"].as_i64()?;
            Some(KeyRow {
                id,
                name: v["name"].as_str().unwrap_or_default().to_string(),
                prefix: v["prefix"].as_str().unwrap_or_default().to_string(),
                scopes: v["scopes"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|s| s.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                created_at: v["created_at"].as_i64(),
                last_used_at: used.get(&id).copied(),
            })
        })
        .collect();
    rows.sort_by_key(|r| r.id);
    Ok(rows)
}

/// What a reset did.
#[derive(Debug, Serialize)]
pub struct ResetDone {
    pub sessions_revoked: usize,
    pub agent_keys_revoked: usize,
}

pub enum ResetRefused {
    /// The new password fails the password rules; the reply says why.
    Weak(axum::response::Response),
    /// The keys named are no longer the account's keys; here are the
    /// current ones to show again.
    KeyListChanged(Vec<KeyRow>),
    Status(StatusCode),
}

impl From<StatusCode> for ResetRefused {
    fn from(s: StatusCode) -> Self {
        Self::Status(s)
    }
}

/// Set `address`'s password to `password`, sign its sessions out, and,
/// when `compromised` names the keys the administrator was shown, revoke
/// every agent key it has. Audited as `actor`; `via` says which surface.
pub(crate) async fn reset_password(
    state: &Arc<WebState>,
    actor: &str,
    address: &str,
    password: &str,
    compromised: Option<&[i64]>,
    via: &str,
) -> Result<ResetDone, ResetRefused> {
    use argon2::{
        Argon2,
        password_hash::{PasswordHasher, SaltString, rand_core::OsRng as ArgonRng},
    };
    crate::handlers::grants::require_may_touch(state, actor, address).await?;
    if let Err(weak) = mailrs_auth_guard::password::check_new(password, address) {
        return Err(ResetRefused::Weak(
            crate::handlers::auth::password::refused(weak),
        ));
    }
    if let Some(named) = compromised {
        let addr = address.to_string();
        let rows = with_kevy(move |c| key_rows(c, &addr))?;
        let mut named = named.to_vec();
        named.sort_unstable();
        named.dedup();
        if named != rows.iter().map(|r| r.id).collect::<Vec<_>>() {
            return Err(ResetRefused::KeyListChanged(rows));
        }
    }
    let salt = SaltString::generate(&mut ArgonRng);
    let password_hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .to_string();
    let req = mailrs_core_api::method::admin::SetPasswordRequest { password_hash };
    state
        .core
        .set_account_password(address, &req)
        .await
        .map_err(map_err)?;
    let addr = address.to_string();
    let revoke_keys = compromised.is_some();
    let (sessions, revoked) = with_kevy(move |c| {
        let sessions = crate::handlers::auth::revoke_sessions(c, &addr)?;
        let revoked = match revoke_keys {
            true => revoke_every_key(c, &addr)?,
            false => Vec::new(),
        };
        Ok((sessions, revoked))
    })?;
    for key in &revoked {
        let detail = format!(
            "id={} prefix={} compromise reset by {actor}",
            key.id, key.prefix
        );
        crate::handlers::audit::record(actor, "agent_key.revoke", address, &detail);
    }
    let mut detail = via.to_string();
    if revoke_keys {
        detail = format!("{detail} compromise, keys={}", revoked.len());
    }
    crate::handlers::audit::record(actor, "account.password_reset", address, detail.trim());
    Ok(ResetDone {
        sessions_revoked: sessions,
        agent_keys_revoked: revoked.len(),
    })
}

/// Revoke `address`'s keys until none is left: a key made by a request
/// still in flight while the first ones went is revoked as well.
fn revoke_every_key(c: &mut Connection, address: &str) -> std::io::Result<Vec<KeyRow>> {
    let mut revoked = Vec::new();
    loop {
        let rows = key_rows(c, address)?;
        if rows.is_empty() {
            return Ok(revoked);
        }
        for row in rows {
            crate::handlers::apps_keys::revoke_agent_key(c, address, row.id)?;
            revoked.push(row);
        }
    }
}
