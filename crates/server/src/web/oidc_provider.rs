use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect};
use axum::Json;
use chrono::{Duration, Utc};
use serde::Deserialize;

use super::WebState;
use crate::oidc_jwt;
use crate::oidc_store;

// --- Discovery ---

pub(super) async fn openid_configuration(
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
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

pub(super) async fn jwks(State(state): State<Arc<WebState>>) -> impl IntoResponse {
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

// --- Authorize ---

#[derive(Deserialize)]
pub(super) struct AuthorizeQuery {
    pub client_id: String,
    pub redirect_uri: String,
    pub response_type: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub nonce: Option<String>,
    #[serde(default)]
    pub code_challenge: Option<String>,
    #[serde(default)]
    pub code_challenge_method: Option<String>,
}

pub(super) async fn authorize(
    Query(params): Query<AuthorizeQuery>,
    State(state): State<Arc<WebState>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // validate response_type
    if params.response_type != "code" {
        return error_redirect_or_json(
            &params.redirect_uri,
            params.state.as_deref(),
            "unsupported_response_type",
            "only response_type=code is supported",
        );
    }

    let Some(ref pool) = state.pg_pool else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database not configured"})),
        )
            .into_response();
    };

    // validate client
    let client = match oidc_store::get_client(pool, &params.client_id).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid_client", "error_description": "client not found"})),
            )
                .into_response();
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to get oauth client");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "server_error"})),
            )
                .into_response();
        }
    };

    // validate redirect_uri
    if !client.redirect_uris.contains(&params.redirect_uri) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid_request",
                "error_description": "redirect_uri not in allowed list"
            })),
        )
            .into_response();
    }

    // try to extract session (optional — no 401 if missing)
    let session = try_extract_session(&state, &headers);

    match session {
        None => {
            // no session — redirect to login
            let authorize_url = build_authorize_url(&state.hostname, &params);
            let encoded = urlencoding::encode(&authorize_url);
            let login_url = format!("/login?return_to={encoded}");
            Redirect::temporary(&login_url).into_response()
        }
        Some((address, display_name)) => {
            if client.trusted {
                // auto-approve for trusted clients
                let code = generate_auth_code();
                let scope = if params.scope.is_empty() {
                    "openid email profile".to_string()
                } else {
                    params.scope.clone()
                };
                let expires_at = Utc::now() + Duration::seconds(600);

                if let Err(e) = oidc_store::store_auth_code(
                    pool,
                    &code,
                    &params.client_id,
                    &address,
                    &params.redirect_uri,
                    &scope,
                    params.code_challenge.as_deref(),
                    params.code_challenge_method.as_deref(),
                    params.nonce.as_deref(),
                    expires_at,
                )
                .await
                {
                    tracing::warn!(error = %e, "failed to store auth code");
                    return error_redirect_or_json(
                        &params.redirect_uri,
                        params.state.as_deref(),
                        "server_error",
                        "failed to generate authorization code",
                    );
                }

                let mut redirect = format!("{}?code={}", params.redirect_uri, code);
                if let Some(ref st) = params.state {
                    redirect.push_str(&format!("&state={}", urlencoding::encode(st)));
                }
                Redirect::temporary(&redirect).into_response()
            } else {
                // untrusted client — return consent info as JSON
                (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "consent_required": true,
                        "client_name": client.name,
                        "scopes": params.scope,
                        "user": address,
                        "display_name": display_name,
                    })),
                )
                    .into_response()
            }
        }
    }
}

// --- Token ---

pub(super) async fn token(
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
pub(super) struct TokenRequest {
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

pub(super) async fn userinfo(
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
            Json(serde_json::json!({"error": "invalid_token", "error_description": "missing bearer token"})),
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
                Json(serde_json::json!({"error": "invalid_token", "error_description": "token verification failed"})),
            );
        }
    };

    let Some(sub) = claims["sub"].as_str() else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid_token", "error_description": "missing sub claim"})),
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

// --- Helpers ---

/// try to extract a session from Authorization header without returning 401
fn try_extract_session(
    state: &Arc<WebState>,
    headers: &axum::http::HeaderMap,
) -> Option<(String, String)> {
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let token = auth_header.strip_prefix("Bearer ")?;

    // only check session tokens (not API keys)
    if token.starts_with("mlrs_") {
        return None;
    }

    let session = state.sessions.get(token)?;
    if session.created_at.elapsed() < super::SESSION_TTL {
        Some((session.address.clone(), session.display_name.clone()))
    } else {
        None
    }
}

/// generate a random authorization code (32 bytes hex-encoded)
fn generate_auth_code() -> String {
    oidc_store::generate_random_hex(32)
}

/// build the full authorize URL from params (for return_to)
fn build_authorize_url(hostname: &str, params: &AuthorizeQuery) -> String {
    let mut url = format!(
        "https://{}/oauth/authorize?client_id={}&redirect_uri={}&response_type={}",
        hostname,
        urlencoding::encode(&params.client_id),
        urlencoding::encode(&params.redirect_uri),
        urlencoding::encode(&params.response_type),
    );
    if !params.scope.is_empty() {
        url.push_str(&format!("&scope={}", urlencoding::encode(&params.scope)));
    }
    if let Some(ref st) = params.state {
        url.push_str(&format!("&state={}", urlencoding::encode(st)));
    }
    if let Some(ref n) = params.nonce {
        url.push_str(&format!("&nonce={}", urlencoding::encode(n)));
    }
    if let Some(ref cc) = params.code_challenge {
        url.push_str(&format!("&code_challenge={}", urlencoding::encode(cc)));
    }
    if let Some(ref ccm) = params.code_challenge_method {
        url.push_str(&format!(
            "&code_challenge_method={}",
            urlencoding::encode(ccm)
        ));
    }
    url
}

/// PKCE S256: SHA-256 hash of verifier, base64url-encoded without padding
fn pkce_s256(verifier: &str) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use sha2::{Digest, Sha256};

    let hash = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
}

/// return an error as either a redirect (with error params) or JSON
fn error_redirect_or_json(
    redirect_uri: &str,
    state: Option<&str>,
    error: &str,
    description: &str,
) -> axum::response::Response {
    if redirect_uri.starts_with("http") {
        let mut url = format!(
            "{}?error={}&error_description={}",
            redirect_uri,
            urlencoding::encode(error),
            urlencoding::encode(description),
        );
        if let Some(st) = state {
            url.push_str(&format!("&state={}", urlencoding::encode(st)));
        }
        Redirect::temporary(&url).into_response()
    } else {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": error,
                "error_description": description,
            })),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkce_s256() {
        // RFC 7636 test vector
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let expected = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert_eq!(pkce_s256(verifier), expected);
    }

    #[test]
    fn test_generate_auth_code_length() {
        let code = generate_auth_code();
        assert_eq!(code.len(), 64); // 32 bytes = 64 hex chars
    }

    #[test]
    fn test_generate_auth_code_uniqueness() {
        let a = generate_auth_code();
        let b = generate_auth_code();
        assert_ne!(a, b);
    }
}
