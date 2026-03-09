use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use rand_core::RngCore;
use serde::Deserialize;

use crate::inbound::auth_guard::AuthCheck;

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
}

/// extractor that validates bearer token and returns the authenticated user context
pub(crate) struct AuthUser {
    pub address: String,
    pub display_name: String,
    pub super_domains: Vec<String>,
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

        if let Some(token) = token {
            if let Some(session) = state.sessions.get(token.as_str()) {
                if session.created_at.elapsed() < super::SESSION_TTL {
                    return Ok(AuthUser {
                        address: session.address.clone(),
                        display_name: session.display_name.clone(),
                        super_domains: session.super_domains.clone(),
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

    // generate token
    let mut bytes = [0u8; 32];
    rand_core::OsRng.fill_bytes(&mut bytes);
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();

    let super_domains: Vec<String> = account
        .super_domains
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    state.sessions.insert(
        token.clone(),
        SessionInfo {
            address: account.address.clone(),
            display_name: account.display_name.clone(),
            super_domains: super_domains.clone(),
            created_at: std::time::Instant::now(),
        },
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "token": token,
            "address": account.address,
            "display_name": account.display_name,
            "super_domains": super_domains,
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
    AuthUser { address, display_name, super_domains, .. }: AuthUser,
) -> impl IntoResponse {
    Json(serde_json::json!({
        "address": address,
        "display_name": display_name,
        "super_domains": super_domains,
    }))
}
