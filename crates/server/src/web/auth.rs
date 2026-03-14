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
        if let Some(ref ds) = state.domain_store {
            ds.log_audit(&req.address, "login_failed", "", &format!("ip={}", addr.ip())).await;
        }
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid credentials"})),
        );
    }

    if let Some(ref guard) = state.auth_guard {
        guard.record_success(addr.ip(), &req.address);
    }

    // audit log successful login
    if let Some(ref ds) = state.domain_store {
        ds.log_audit(&req.address, "login", "", &format!("ip={}", addr.ip())).await;
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
            "send_as": permissions.send_as(),
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
        "send_as": permissions.send_as(),
    }))
}

#[derive(Deserialize)]
pub(super) struct ForgotPasswordRequest {
    pub address: String,
}

#[derive(Deserialize)]
pub(super) struct ResetPasswordRequest {
    pub token: String,
    pub new_password: String,
}

pub(super) async fn forgot_password(
    State(state): State<Arc<WebState>>,
    Json(req): Json<ForgotPasswordRequest>,
) -> impl IntoResponse {
    if req.address.is_empty() || req.address.len() > super::MAX_ADMIN_FIELD_LEN {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid address"})),
        );
    }

    let Some(ref pool) = state.pg_pool else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "auth backend unavailable"})),
        );
    };

    // verify account exists (but always return success to prevent enumeration)
    let account_exists = if let Some(ref ds) = state.domain_store {
        ds.get_account_with_hash(&req.address)
            .await
            .ok()
            .flatten()
            .is_some()
    } else {
        false
    };

    if account_exists {
        // generate reset token
        let mut bytes = [0u8; 32];
        rand_core::OsRng.fill_bytes(&mut bytes);
        let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();

        let expires_at = Utc::now() + chrono::Duration::hours(1);

        let inserted = sqlx::query(
            "INSERT INTO password_reset_tokens (account_address, token, expires_at) \
             VALUES ($1, $2, $3)",
        )
        .bind(&req.address)
        .bind(&token)
        .bind(expires_at)
        .execute(pool)
        .await;

        if inserted.is_ok() {
            // send reset email to the user's mailbox
            let reset_link = format!("https://mail.golia.ai/reset-password?token={token}");
            let subject = "Password Reset Request";
            let body = format!(
                "You requested a password reset.\n\n\
                 Click the link below to reset your password:\n\
                 {reset_link}\n\n\
                 This link expires in 1 hour.\n\n\
                 If you did not request this, please ignore this email."
            );

            let now = Utc::now();
            let message_id = format!(
                "{}.{}@{}",
                now.timestamp_millis(),
                rand_core::OsRng.next_u32(),
                state.hostname
            );
            let from = format!("noreply@{}", state.hostname);
            let to = vec![req.address.clone()];
            let raw = super::mail::build_rfc5322_message(
                &from,
                &to,
                &[],
                subject,
                &body,
                None,
                &message_id,
                None,
                &[],
                &now,
                None,
            );

            // deliver directly to local mailbox
            if let Some(ref mb_store) = state.mailbox_store {
                let _ = mb_store.ensure_default_mailboxes(&req.address).await;
                let _ = mb_store
                    .append_message(
                        &req.address,
                        "INBOX",
                        &state.maildir_root,
                        &raw,
                        0,
                        now.timestamp(),
                    )
                    .await;
            }

            if let Some(ref ds) = state.domain_store {
                ds.log_audit(&req.address, "password_reset_requested", &req.address, "")
                    .await;
            }
        }
    }

    // always return success to prevent account enumeration
    (
        StatusCode::OK,
        Json(serde_json::json!({"success": true})),
    )
}

pub(super) async fn reset_password(
    State(state): State<Arc<WebState>>,
    Json(req): Json<ResetPasswordRequest>,
) -> impl IntoResponse {
    if req.token.is_empty() || req.token.len() > 128 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid token"})),
        );
    }

    if let Err(e) = crate::users::validate_password(&req.new_password) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        );
    }

    let Some(ref pool) = state.pg_pool else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "auth backend unavailable"})),
        );
    };

    // look up token
    let row: Option<(i64, String, bool)> = sqlx::query_as(
        "SELECT id, account_address, used FROM password_reset_tokens \
         WHERE token = $1 AND expires_at > now()",
    )
    .bind(&req.token)
    .fetch_optional(pool)
    .await
    .unwrap_or(None);

    let Some((token_id, account_address, used)) = row else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid or expired token"})),
        );
    };

    if used {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "token already used"})),
        );
    }

    // hash new password
    let password_hash = match crate::users::UserStore::hash_password(&req.new_password) {
        Ok(h) => h,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "failed to hash password"})),
            );
        }
    };

    // update password via domain store
    let Some(ref ds) = state.domain_store else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "auth backend unavailable"})),
        );
    };

    // get current account to preserve domain and display_name
    let (account, _) = match ds.get_account_with_hash(&account_address).await {
        Ok(Some(pair)) => pair,
        _ => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "account not found"})),
            );
        }
    };

    let now = Utc::now().timestamp();
    if let Err(e) = ds
        .add_account(
            &account.address,
            &account.domain,
            &account.display_name,
            &password_hash,
            now,
        )
        .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("failed to update password: {e}")})),
        );
    }

    // mark token as used
    let _ = sqlx::query("UPDATE password_reset_tokens SET used = true WHERE id = $1")
        .bind(token_id)
        .execute(pool)
        .await;

    // audit log
    ds.log_audit(&account_address, "password_reset", &account_address, "")
        .await;

    (
        StatusCode::OK,
        Json(serde_json::json!({"success": true})),
    )
}
