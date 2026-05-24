//! API-key verification middleware-helper for MCP + REST.

#![allow(unused_imports)]

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Json, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};

use crate::inbound::auth_guard::AuthCheck;

use super::super::{AuthUser, SessionInfo, WebState};
use super::*;

/// verify an API key token (mlrs_{prefix}_{secret}) against cache/DB
pub(super) async fn verify_api_key(
    token: &str,
    state: &Arc<WebState>,
) -> Result<AuthUser, (StatusCode, &'static str)> {
    // parse: mlrs_{8hex}_{40hex}
    let parts: Vec<&str> = token.splitn(3, '_').collect();
    if parts.len() != 3 || parts[0] != "mlrs" {
        return Err((StatusCode::UNAUTHORIZED, "invalid api key format"));
    }
    let prefix = parts[1];

    // try Valkey cache first
    let cached = if let Some(ref valkey) = state.valkey {
        api_key_store::cache_get(valkey, prefix).await
    } else {
        None
    };

    let cached = match cached {
        Some(c) => c,
        None => {
            // cache miss — query PG
            let pool = state
                .pg_pool
                .as_ref()
                .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "auth backend unavailable"))?;

            let record = api_key_store::get_api_key_by_prefix(pool, prefix)
                .await
                .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "auth backend unavailable"))?
                .ok_or((StatusCode::UNAUTHORIZED, "invalid api key"))?;

            let entry = api_key_store::CachedApiKey {
                key_hash: record.key_hash,
                account_address: record.account_address,
                expires_at: record.expires_at,
                id: record.id,
                app_id: record.app_id,
            };

            // populate cache
            if let Some(ref valkey) = state.valkey {
                api_key_store::cache_set(valkey, prefix, &entry).await;
            }

            entry
        }
    };

    // verify hash
    let token_hash = api_key_store::sha256_hex(token.as_bytes());
    if token_hash != cached.key_hash {
        return Err((StatusCode::UNAUTHORIZED, "invalid api key"));
    }

    // check expiration
    if let Some(expires_at) = cached.expires_at
        && expires_at < Utc::now() {
            return Err((StatusCode::UNAUTHORIZED, "api key expired"));
        }

    // fire-and-forget last_used_at update
    if let Some(ref pool) = state.pg_pool {
        let pool = pool.clone();
        let id = cached.id;
        tokio::spawn(async move {
            api_key_store::update_last_used(&pool, id).await;
        });
    }

    // resolve display_name and permissions
    let (display_name, permissions, auth_method) = if let Some(app_id) = cached.app_id {
        // app key: permissions come from app scopes
        if let Some(ref ds) = state.domain_store {
            let app = ds.get_app_by_id(app_id).await.ok().flatten();
            match app {
                Some(app) if app.active => {
                    let scopes: Vec<String> = app
                        .scopes
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    let all_domains: Vec<String> = ds
                        .list_domains()
                        .await
                        .unwrap_or_default()
                        .into_iter()
                        .map(|d| d.name)
                        .collect();
                    let perms = crate::permission::from_scopes(&scopes, &all_domains);
                    (
                        app.name.clone(),
                        perms,
                        AuthMethod::AppKey(cached.id, app_id),
                    )
                }
                _ => {
                    return Err((StatusCode::UNAUTHORIZED, "app disabled or not found"));
                }
            }
        } else {
            return Err((StatusCode::INTERNAL_SERVER_ERROR, "auth backend unavailable"));
        }
    } else {
        // user key: permissions come from account groups
        if let Some(ref ds) = state.domain_store {
            let dn = match ds.get_account_with_hash(&cached.account_address).await {
                Ok(Some((account, _))) => account.display_name,
                _ => cached.account_address.clone(),
            };
            let perms = ds
                .load_account_permissions(&cached.account_address)
                .await
                .unwrap_or_else(|_| {
                    crate::permission::compute_effective_permissions(&[], &[], &[])
                });
            (dn, perms, AuthMethod::ApiKey(cached.id))
        } else {
            (
                cached.account_address.clone(),
                crate::permission::compute_effective_permissions(&[], &[], &[]),
                AuthMethod::ApiKey(cached.id),
            )
        }
    };

    Ok(AuthUser {
        address: cached.account_address,
        display_name,
        permissions: Arc::new(permissions),
        auth_method,
    })
}
