//! OIDC token endpoint: auth_code + refresh_token grants + JWT issuance.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{Duration, Utc};
use serde::Deserialize;

use super::super::WebState;
use crate::oidc_jwt;
use crate::oidc_store;

pub(crate) async fn token(
    State(state): State<Arc<WebState>>,
    axum::Form(form): axum::Form<TokenRequest>,
) -> impl IntoResponse {
    let Some(ref pool) = state.pg_pool else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "server_error"})),
        );
    };

    match form.grant_type.as_str() {
        "authorization_code" => handle_authorization_code_grant(pool, &state, &form).await,
        "refresh_token" => handle_refresh_token_grant(pool, &state, &form).await,
        _ => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "unsupported_grant_type",
                "error_description": "only authorization_code and refresh_token are supported"
            })),
        ),
    }
}

#[derive(Deserialize)]
pub(crate) struct TokenRequest {
    pub grant_type: String,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub redirect_uri: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub code_verifier: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
}

async fn handle_authorization_code_grant(
    pool: &sqlx::PgPool,
    state: &Arc<WebState>,
    form: &TokenRequest,
) -> (StatusCode, Json<serde_json::Value>) {
    let (Some(code), Some(redirect_uri), Some(client_id), Some(client_secret)) = (
        form.code.as_deref(),
        form.redirect_uri.as_deref(),
        form.client_id.as_deref(),
        form.client_secret.as_deref(),
    ) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid_request",
                "error_description": "missing required parameters"
            })),
        );
    };

    // validate client credentials
    let client = match oidc_store::get_client(pool, client_id).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "invalid_client"})),
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to get oauth client");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "server_error"})),
            );
        }
    };

    if !oidc_store::verify_client_secret(client_secret, &client.secret_hash) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid_client", "error_description": "invalid client credentials"})),
        );
    }

    // consume auth code atomically
    let auth_code = match oidc_store::consume_auth_code(pool, code).await {
        Ok(Some(ac)) => ac,
        Ok(None) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid_grant", "error_description": "code is invalid, expired, or already used"})),
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to consume auth code");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "server_error"})),
            );
        }
    };

    // verify redirect_uri matches
    if auth_code.redirect_uri != redirect_uri {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid_grant", "error_description": "redirect_uri mismatch"})),
        );
    }

    // verify client_id matches
    if auth_code.client_id != client_id {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid_grant", "error_description": "client_id mismatch"})),
        );
    }

    // PKCE verification
    if let Some(ref challenge) = auth_code.code_challenge {
        let Some(ref verifier) = form.code_verifier else {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid_grant", "error_description": "code_verifier required"})),
            );
        };

        let method = auth_code
            .code_challenge_method
            .as_deref()
            .unwrap_or("S256");
        if method != "S256" {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid_request", "error_description": "unsupported code_challenge_method"})),
            );
        }

        let computed = pkce_s256(verifier);
        if computed != *challenge {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid_grant", "error_description": "code_verifier does not match"})),
            );
        }
    }

    // generate tokens
    issue_tokens(
        pool,
        state,
        client_id,
        &auth_code.account_address,
        &auth_code.scopes,
        auth_code.nonce.as_deref(),
    )
    .await
}

async fn handle_refresh_token_grant(
    pool: &sqlx::PgPool,
    state: &Arc<WebState>,
    form: &TokenRequest,
) -> (StatusCode, Json<serde_json::Value>) {
    let (Some(client_id), Some(client_secret), Some(refresh_token)) = (
        form.client_id.as_deref(),
        form.client_secret.as_deref(),
        form.refresh_token.as_deref(),
    ) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid_request",
                "error_description": "missing required parameters"
            })),
        );
    };

    // validate client
    let client = match oidc_store::get_client(pool, client_id).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "invalid_client"})),
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to get oauth client");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "server_error"})),
            );
        }
    };

    if !oidc_store::verify_client_secret(client_secret, &client.secret_hash) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid_client"})),
        );
    }

    // validate refresh token
    let token_hash = oidc_store::sha256_hex(refresh_token.as_bytes());
    let rt = match oidc_store::validate_refresh_token(pool, &token_hash).await {
        Ok(Some(rt)) => rt,
        Ok(None) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid_grant", "error_description": "refresh token invalid or expired"})),
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to validate refresh token");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "server_error"})),
            );
        }
    };

    // verify client_id matches
    if rt.client_id != client_id {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid_grant", "error_description": "client_id mismatch"})),
        );
    }

    // revoke old refresh token (rotation)
    let _ = oidc_store::revoke_refresh_token(pool, &token_hash).await;

    // issue new tokens
    issue_tokens(
        pool,
        state,
        client_id,
        &rt.account_address,
        &rt.scopes,
        None,
    )
    .await
}

/// issue id_token, access_token, and refresh_token
async fn issue_tokens(
    pool: &sqlx::PgPool,
    state: &Arc<WebState>,
    client_id: &str,
    account_address: &str,
    scopes: &str,
    nonce: Option<&str>,
) -> (StatusCode, Json<serde_json::Value>) {
    let signing_key = match oidc_store::get_active_signing_key(pool).await {
        Ok(Some(k)) => k,
        Ok(None) => {
            tracing::warn!("no active signing key for oidc token issuance");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "server_error"})),
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to get signing key");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "server_error"})),
            );
        }
    };

    let issuer = format!("https://{}", state.hostname);
    let now = Utc::now().timestamp();
    let exp = now + 300; // 5 minutes

    // look up display_name
    let display_name = if let Some(ref ds) = state.domain_store {
        match ds.get_account_with_hash(account_address).await {
            Ok(Some((account, _))) => account.display_name,
            _ => account_address.to_string(),
        }
    } else {
        account_address.to_string()
    };

    // id_token claims
    let mut id_claims = serde_json::json!({
        "iss": issuer,
        "sub": account_address,
        "aud": client_id,
        "exp": exp,
        "iat": now,
        "email": account_address,
        "name": display_name,
        "email_verified": true,
    });
    if let Some(nonce) = nonce {
        id_claims["nonce"] = serde_json::json!(nonce);
    }

    let id_token = match oidc_jwt::sign_jwt(
        &signing_key.private_key_pem,
        &signing_key.kid,
        &id_claims,
    ) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(error = %e, "failed to sign id_token");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "server_error"})),
            );
        }
    };

    // access_token claims
    let access_claims = serde_json::json!({
        "iss": issuer,
        "sub": account_address,
        "aud": client_id,
        "exp": exp,
        "iat": now,
        "scope": scopes,
    });

    let access_token = match oidc_jwt::sign_jwt(
        &signing_key.private_key_pem,
        &signing_key.kid,
        &access_claims,
    ) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(error = %e, "failed to sign access_token");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "server_error"})),
            );
        }
    };

    // refresh_token: 64 random bytes hex
    let refresh_token_raw = oidc_store::generate_random_hex(64);
    let refresh_token_hash = oidc_store::sha256_hex(refresh_token_raw.as_bytes());
    let refresh_expires = Utc::now() + Duration::days(7);

    if let Err(e) = oidc_store::store_refresh_token(
        pool,
        &refresh_token_hash,
        client_id,
        account_address,
        scopes,
        refresh_expires,
    )
    .await
    {
        tracing::warn!(error = %e, "failed to store refresh token");
        // still return access + id tokens even if refresh token storage fails
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "access_token": access_token,
            "token_type": "Bearer",
            "expires_in": 300,
            "id_token": id_token,
            "refresh_token": refresh_token_raw,
        })),
    )
}

// --- UserInfo ---


fn pkce_s256(verifier: &str) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use sha2::{Digest, Sha256};

    let hash = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
}

/// return an error as either a redirect (with error params) or JSON

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkce_s256() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let expected = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert_eq!(pkce_s256(verifier), expected);
    }
}
