use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::api_key_store;
use crate::web::{AuthMethod, AuthUser, WebState};

/// middleware that validates Bearer token for MCP routes
/// on success, inserts AuthUser into request extensions
/// on failure, returns 401 Unauthorized
///
/// The `mcp.request` span here is the only MCP-level tracing
/// instrumentation. Per-tool spans would require wiring
/// `#[tracing::instrument]` to each `#[tool(...)]` method on
/// `MailMcpService`, but the `rmcp::tool_router` macro currently
/// rejects stacked attributes — so per-tool observability is
/// approximated via the `tool` request field on the JSON-RPC
/// envelope (visible in span fields after auth resolves).
#[tracing::instrument(
    name = "mcp.request",
    skip(state, request, next),
    fields(user = tracing::field::Empty, prefix = tracing::field::Empty)
)]
pub(crate) async fn mcp_auth_middleware(
    State(state): State<Arc<WebState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let auth_header = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let Some(token) = auth_header.strip_prefix("Bearer ") else {
        return (StatusCode::UNAUTHORIZED, "missing bearer token").into_response();
    };

    if !token.starts_with("mlrs_") {
        return (StatusCode::UNAUTHORIZED, "invalid token format").into_response();
    }

    // parse: mlrs_{8hex}_{40hex}
    let parts: Vec<&str> = token.splitn(3, '_').collect();
    if parts.len() != 3 || parts[0] != "mlrs" {
        return (StatusCode::UNAUTHORIZED, "invalid api key format").into_response();
    }
    let prefix = parts[1];

    // try valkey cache first
    let cached = if let Some(ref valkey) = state.valkey {
        api_key_store::cache_get(valkey, prefix).await
    } else {
        None
    };

    let cached = match cached {
        Some(c) => c,
        None => {
            // cache miss — query PG
            let Some(ref pool) = state.pg_pool else {
                return (StatusCode::INTERNAL_SERVER_ERROR, "auth backend unavailable").into_response();
            };

            let record = match api_key_store::get_api_key_by_prefix(pool, prefix).await {
                Ok(Some(r)) => r,
                Ok(None) => {
                    return (StatusCode::UNAUTHORIZED, "invalid api key").into_response();
                }
                Err(_) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, "auth backend unavailable").into_response();
                }
            };

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
        return (StatusCode::UNAUTHORIZED, "invalid api key").into_response();
    }

    // check expiration
    if let Some(expires_at) = cached.expires_at
        && expires_at < chrono::Utc::now() {
            return (StatusCode::UNAUTHORIZED, "api key expired").into_response();
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
        // app key
        if let Some(ref ds) = state.domain_store {
            let app = ds.get_app_by_id(app_id).await.ok().flatten();
            match app {
                Some(app) if app.active => {
                    let scopes: Vec<String> = app.scopes.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    let all_domains: Vec<String> = ds.list_domains().await
                        .unwrap_or_default().into_iter().map(|d| d.name).collect();
                    let perms = crate::permission::from_scopes(&scopes, &all_domains);
                    (app.name.clone(), perms, AuthMethod::AppKey(cached.id, app_id))
                }
                _ => return (StatusCode::UNAUTHORIZED, "app disabled or not found").into_response(),
            }
        } else {
            return (StatusCode::INTERNAL_SERVER_ERROR, "auth backend unavailable").into_response();
        }
    } else {
        // user key
        if let Some(ref ds) = state.domain_store {
            let dn = match ds.get_account_with_hash(&cached.account_address).await {
                Ok(Some((account, _))) => account.display_name,
                _ => cached.account_address.clone(),
            };
            let perms = ds.load_account_permissions(&cached.account_address).await
                .unwrap_or_else(|_| crate::permission::compute_effective_permissions(&[], &[], &[]));
            (dn, perms, AuthMethod::ApiKey(cached.id))
        } else {
            (
                cached.account_address.clone(),
                crate::permission::compute_effective_permissions(&[], &[], &[]),
                AuthMethod::ApiKey(cached.id),
            )
        }
    };

    let auth_user = AuthUser {
        address: cached.account_address,
        display_name,
        permissions: std::sync::Arc::new(permissions),
        auth_method,
    };
    tracing::Span::current().record("user", auth_user.address.as_str());
    tracing::Span::current().record("prefix", prefix);

    request.extensions_mut().insert(auth_user.clone());
    // set task-local so the StreamableHttpService factory can read it
    super::MCP_AUTH_USER.scope(auth_user, next.run(request)).await
}
