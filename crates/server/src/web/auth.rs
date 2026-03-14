use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use rand_core::RngCore;
use serde::Deserialize;

use crate::api_key_store;
use crate::inbound::auth_guard::AuthCheck;
use crate::permission::EffectivePermissions;

use super::{ApiResult, SessionInfo, WebState};

#[derive(Deserialize)]
pub(super) struct LoginRequest {
    pub address: String,
    pub password: String,
}

/// how the user authenticated
#[derive(Debug, Clone)]
pub(crate) enum AuthMethod {
    Session,
    ApiKey(i64),
    /// app key: (api_key_id, app_internal_id)
    AppKey(i64, i64),
}

/// extractor that validates bearer token and returns the authenticated user context
#[derive(Debug, Clone)]
pub(crate) struct AuthUser {
    pub address: String,
    pub display_name: String,
    pub permissions: Arc<EffectivePermissions>,
    pub auth_method: AuthMethod,
}

impl FromRequestParts<Arc<WebState>> for AuthUser {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<WebState>,
    ) -> Result<Self, Self::Rejection> {
        // try Authorization header first
        let auth_header = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let token = if let Some(t) = auth_header.strip_prefix("Bearer ") {
            Some(t.to_string())
        } else {
            // fallback: ?token= query param (for <img src>, <a href>, <iframe src>)
            parts
                .uri
                .query()
                .and_then(|q| {
                    q.split('&')
                        .find_map(|pair| pair.strip_prefix("token="))
                        .map(|t| t.to_string())
                })
        };

        if let Some(ref token) = token {
            if token.starts_with("mlrs_") {
                return verify_api_key(token, state).await;
            }

            if let Some(session) = state.sessions.get(token.as_str()) {
                if session.created_at.elapsed() < super::SESSION_TTL {
                    return Ok(AuthUser {
                        address: session.address.clone(),
                        display_name: session.display_name.clone(),
                        permissions: session.permissions.clone(),
                        auth_method: AuthMethod::Session,
                    });
                }
                drop(session);
                state.sessions.remove(token.as_str());
            }
        }

        Err((StatusCode::UNAUTHORIZED, "authentication required"))
    }
}

/// verify an API key token (mlrs_{prefix}_{secret}) against cache/DB
async fn verify_api_key(
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
    if let Some(expires_at) = cached.expires_at {
        if expires_at < Utc::now() {
            return Err((StatusCode::UNAUTHORIZED, "api key expired"));
        }
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

pub(super) async fn login(
    State(state): State<Arc<WebState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<LoginRequest>,
) -> impl IntoResponse {
    if req.address.len() > super::MAX_ADMIN_FIELD_LEN || req.password.len() > 1024 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid input length"})),
        );
    }

    let Some(ref ds) = state.domain_store else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "auth not configured"})),
        );
    };

    // check auth guard before attempting verification
    if let Some(ref guard) = state.auth_guard {
        if let AuthCheck::LockedOut { remaining_secs } = guard.check(addr.ip(), &req.address) {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({
                    "error": format!("Too many auth failures, try again in {remaining_secs}s")
                })),
            );
        }
    }

    let (account, password_hash) = match ds.get_account_with_hash(&req.address).await {
        Ok(Some(pair)) => pair,
        _ => {
            if let Some(ref guard) = state.auth_guard {
                guard.record_failure(addr.ip(), &req.address);
            }
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "invalid credentials"})),
            );
        }
    };

    if !account.active {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "account disabled"})),
        );
    }

    // verify password
    let valid = if password_hash.starts_with("$argon2") {
        crate::users::UserStore::verify_hash(&req.password, &password_hash)
    } else {
        password_hash == req.password
    };

    if !valid {
        if let Some(ref guard) = state.auth_guard {
            guard.record_failure(addr.ip(), &req.address);
        }
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid credentials"})),
        );
    }

    if let Some(ref guard) = state.auth_guard {
        guard.record_success(addr.ip(), &req.address);
    }

    // load permissions
    let permissions = if let Some(ref ds) = state.domain_store {
        Arc::new(
            ds.load_account_permissions(&account.address)
                .await
                .unwrap_or_else(|_| {
                    crate::permission::compute_effective_permissions(&[], &[], &[])
                }),
        )
    } else {
        Arc::new(crate::permission::compute_effective_permissions(
            &[], &[], &[],
        ))
    };

    // generate token
    let mut bytes = [0u8; 32];
    rand_core::OsRng.fill_bytes(&mut bytes);
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();

    state.sessions.insert(
        token.clone(),
        SessionInfo {
            address: account.address.clone(),
            display_name: account.display_name.clone(),
            permissions: permissions.clone(),
            created_at: std::time::Instant::now(),
        },
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "token": token,
            "address": account.address,
            "display_name": account.display_name,
            "permissions": permissions.permission_list(),
            "accessible_domains": permissions.accessible_domains(),
        })),
    )
}

pub(super) async fn logout(
    State(state): State<Arc<WebState>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    if let Some(token) = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        state.sessions.remove(token);
    }
    Json(ApiResult {
        success: true,
        message: None,
    })
}

pub(super) async fn auth_me(
    AuthUser { address, display_name, permissions, .. }: AuthUser,
) -> impl IntoResponse {
    Json(serde_json::json!({
        "address": address,
        "display_name": display_name,
        "permissions": permissions.permission_list(),
        "accessible_domains": permissions.accessible_domains(),
    }))
}
