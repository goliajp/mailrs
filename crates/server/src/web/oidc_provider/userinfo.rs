//! OIDC userinfo endpoint.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;

use super::super::WebState;
use crate::oidc_jwt;
use crate::oidc_store;

pub(crate) async fn userinfo(
    State(state): State<Arc<WebState>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let Some(ref pool) = state.pg_pool else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "server_error"})),
        );
    };

    // extract bearer token
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let Some(token) = auth_header.strip_prefix("Bearer ") else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(
                serde_json::json!({"error": "invalid_token", "error_description": "missing bearer token"}),
            ),
        );
    };

    // get active signing key for verification
    let signing_key = match oidc_store::get_active_signing_key(pool).await {
        Ok(Some(k)) => k,
        Ok(None) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "server_error"})),
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to get signing key for userinfo");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "server_error"})),
            );
        }
    };

    // verify JWT
    let claims = match oidc_jwt::verify_jwt(token, &signing_key.public_key_pem) {
        Ok(c) => c,
        Err(e) => {
            tracing::info!(error = %e, "userinfo jwt verification failed");
            return (
                StatusCode::UNAUTHORIZED,
                Json(
                    serde_json::json!({"error": "invalid_token", "error_description": "token verification failed"}),
                ),
            );
        }
    };

    let Some(sub) = claims["sub"].as_str() else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(
                serde_json::json!({"error": "invalid_token", "error_description": "missing sub claim"}),
            ),
        );
    };

    // look up display_name
    let display_name = if let Some(ref ds) = state.domain_store {
        match ds.get_account_with_hash(sub).await {
            Ok(Some((account, _))) => account.display_name,
            _ => sub.to_string(),
        }
    } else {
        sub.to_string()
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "sub": sub,
            "email": sub,
            "name": display_name,
            "email_verified": true,
        })),
    )
}
