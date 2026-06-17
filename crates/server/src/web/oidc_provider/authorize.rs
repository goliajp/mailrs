//! OIDC authorization endpoint (auth-code flow with PKCE).

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect};
use chrono::{Duration, Utc};
use serde::Deserialize;

use super::super::WebState;
use crate::oidc_store;

#[derive(Deserialize)]
pub(crate) struct AuthorizeQuery {
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

pub(crate) async fn authorize(
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

fn try_extract_session(
    state: &Arc<WebState>,
    headers: &axum::http::HeaderMap,
) -> Option<(String, String)> {
    // try Authorization header first
    let token_from_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(|t| t.to_string());

    // fall back to mailrs_session cookie (set after login for OIDC redirect flow)
    let token_from_cookie = headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|c| {
                c.trim()
                    .strip_prefix("mailrs_session=")
                    .map(|v| v.to_string())
            })
        });

    let token = token_from_header.or(token_from_cookie)?;

    // only check session tokens (not API keys)
    if token.starts_with("mlrs_") {
        return None;
    }

    let session = state.sessions.get(&token)?;
    let elapsed_secs =
        crate::inbound::auth_guard::unix_now().saturating_sub(session.created_at_unix);
    if elapsed_secs < super::super::SESSION_TTL.as_secs() {
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
    fn test_generate_auth_code_length() {
        let code = generate_auth_code();
        assert_eq!(code.len(), 64);
    }

    #[test]
    fn test_generate_auth_code_uniqueness() {
        let a = generate_auth_code();
        let b = generate_auth_code();
        assert_ne!(a, b);
    }
}
