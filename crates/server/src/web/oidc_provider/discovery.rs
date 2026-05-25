//! OIDC discovery: `/.well-known/openid-configuration` + JWKS.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;

use super::super::WebState;
use crate::oidc_jwt;
use crate::oidc_store;

// --- Discovery ---

pub(crate) async fn openid_configuration(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let issuer = format!("https://{}", state.hostname);
    Json(serde_json::json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/oauth/authorize"),
        "token_endpoint": format!("{issuer}/oauth/token"),
        "userinfo_endpoint": format!("{issuer}/oauth/userinfo"),
        "jwks_uri": format!("{issuer}/.well-known/jwks.json"),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["RS256"],
        "scopes_supported": ["openid", "email", "profile"],
        "token_endpoint_auth_methods_supported": ["client_secret_post", "client_secret_basic"],
        "claims_supported": ["sub", "email", "name", "email_verified", "iss", "aud", "exp", "iat", "nonce"],
        "code_challenge_methods_supported": ["S256"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
    }))
}

// --- JWKS ---

pub(crate) async fn jwks(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let Some(ref pool) = state.pg_pool else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database not configured"})),
        );
    };

    let keys = match oidc_store::list_active_public_keys(pool).await {
        Ok(keys) => keys,
        Err(e) => {
            tracing::warn!(error = %e, "failed to list signing keys");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            );
        }
    };

    let mut jwk_keys = Vec::new();
    for key in &keys {
        match oidc_jwt::pem_to_jwk(&key.public_key_pem, &key.kid) {
            Ok(jwk) => jwk_keys.push(jwk),
            Err(e) => {
                tracing::warn!(error = %e, kid = key.kid.as_str(), "failed to convert key to jwk");
            }
        }
    }

    (StatusCode::OK, Json(serde_json::json!({"keys": jwk_keys})))
}
