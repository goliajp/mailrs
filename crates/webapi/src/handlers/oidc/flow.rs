//! The authorization-code flow: /authorize, /token, /userinfo.

use axum::Json;
use axum::extract::{Extension, Form, Query};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect};
use serde::Deserialize;

use super::*;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

#[derive(Debug, Deserialize)]
pub struct AuthorizeQuery {
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

/// GET /oauth/authorize — authenticated user grants the client.
/// Returns 302 back to `redirect_uri` with `code` + `state`.
pub async fn authorize(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Query(q): Query<AuthorizeQuery>,
) -> impl IntoResponse {
    if q.response_type != "code" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "unsupported_response_type"})),
        )
            .into_response();
    }
    // Verify the client is registered AND the redirect_uri exactly
    // matches. Prior version only checked `if let Some(ru)` — an
    // attacker could pass an unregistered client_id and any
    // redirect_uri they wanted, harvesting authorization codes.
    let cid_key = format!("oidc:client:{}", q.client_id);
    let cid_r = cid_key.clone();
    let registered_ru = with_kevy(move |c| {
        c.hget(cid_r.as_bytes(), b"redirect_uri")
            .map_err(std::io::Error::from)
    })
    .ok()
    .flatten()
    .and_then(|v| String::from_utf8(v).ok());
    let Some(ru) = registered_ru else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "unknown_client"})),
        )
            .into_response();
    };
    if ru != q.redirect_uri {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "redirect_uri_mismatch"})),
        )
            .into_response();
    }

    let code = random_hex(24);
    let expires_at = now_secs() + AUTH_CODE_TTL_SECS;
    let code_key = format!("oidc:code:{code}");
    let cc = q.code_challenge.unwrap_or_default();
    let ccm = q.code_challenge_method.unwrap_or_default();
    let nonce = q.nonce.unwrap_or_default();
    let scope = q.scope;
    let redirect_uri = q.redirect_uri.clone();
    let client_id = q.client_id.clone();
    let user_c = user.clone();
    let code_key_c = code_key.clone();
    let _ = with_kevy(move |c| {
        c.hset(
            code_key_c.as_bytes(),
            &[
                (b"client_id" as &[u8], client_id.as_bytes()),
                (b"user", user_c.as_bytes()),
                (b"redirect_uri", redirect_uri.as_bytes()),
                (b"code_challenge", cc.as_bytes()),
                (b"code_challenge_method", ccm.as_bytes()),
                (b"nonce", nonce.as_bytes()),
                (b"scope", scope.as_bytes()),
                (b"expires_at", expires_at.to_string().as_bytes()),
            ],
        )?;
        // Belt-and-braces: also set kevy TTL so a stolen code can't
        // outlast expires_at even if the token endpoint is DoS'd and
        // the exp check is somehow skipped.
        c.expire(
            code_key_c.as_bytes(),
            std::time::Duration::from_secs(AUTH_CODE_TTL_SECS as u64),
        )?;
        Ok(())
    });
    // Percent-encode state so a value containing `&` / `#` / `=` doesn't
    // break the query string. Prior version used raw format! which let
    // an attacker-controlled state inject extra params.
    let redirect = if let Some(state) = q.state {
        let encoded = url::form_urlencoded::byte_serialize(state.as_bytes()).collect::<String>();
        format!("{}?code={code}&state={encoded}", q.redirect_uri)
    } else {
        format!("{}?code={code}", q.redirect_uri)
    };
    Redirect::to(&redirect).into_response()
}

#[derive(Debug, Deserialize)]
pub struct TokenRequest {
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

pub async fn token(Form(req): Form<TokenRequest>) -> impl IntoResponse {
    match req.grant_type.as_str() {
        "authorization_code" => exchange_authorization_code(req).await,
        "refresh_token" => refresh_access_token(req).await,
        _ => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "unsupported_grant_type"})),
        )
            .into_response(),
    }
}

async fn exchange_authorization_code(req: TokenRequest) -> axum::response::Response {
    let Some(code) = req.code else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid_request"})),
        )
            .into_response();
    };
    let code_key = format!("oidc:code:{code}");
    let flat =
        match with_kevy(move |c| c.hgetall(code_key.as_bytes()).map_err(std::io::Error::from)) {
            Ok(v) => v,
            Err(_) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "server_error"})),
                )
                    .into_response();
            }
        };
    if flat.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid_grant"})),
        )
            .into_response();
    }
    let mut fields = std::collections::HashMap::new();
    let mut i = 0;
    while i + 1 < flat.len() {
        fields.insert(
            String::from_utf8_lossy(&flat[i]).to_string(),
            String::from_utf8_lossy(&flat[i + 1]).to_string(),
        );
        i += 2;
    }
    let expires_at: i64 = fields
        .get("expires_at")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    if expires_at < now_secs() {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::json!({"error": "invalid_grant", "error_description": "code expired"}),
            ),
        )
            .into_response();
    }
    // Verify PKCE if the authorize call carried a challenge.
    let cc = fields.get("code_challenge").cloned().unwrap_or_default();
    if !cc.is_empty() {
        let ccm = fields
            .get("code_challenge_method")
            .cloned()
            .unwrap_or_default();
        let Some(verifier) = req.code_verifier else {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid_grant", "error_description": "code_verifier required"})),
            )
                .into_response();
        };
        let derived = match ccm.as_str() {
            "S256" => {
                use sha2::{Digest, Sha256};
                let hash = Sha256::digest(verifier.as_bytes());
                base64_url_encode(&hash)
            }
            "plain" | "" => verifier.clone(),
            _ => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "invalid_grant", "error_description": "unsupported code_challenge_method"})),
                )
                    .into_response();
            }
        };
        if derived != cc {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid_grant", "error_description": "code_verifier mismatch"})),
            )
                .into_response();
        }
    }

    let user = fields.get("user").cloned().unwrap_or_default();
    let scope = fields.get("scope").cloned().unwrap_or_default();
    let client_id = fields.get("client_id").cloned().unwrap_or_default();

    // Verify client_secret unless the client was registered
    // as a public (PKCE-only) client (secret field empty).
    let ci = client_id.clone();
    let registered_secret = with_kevy(move |c| {
        c.hget(format!("oidc:client:{ci}").as_bytes(), b"secret")
            .map_err(std::io::Error::from)
    })
    .ok()
    .flatten()
    .and_then(|v| String::from_utf8(v).ok())
    .unwrap_or_default();
    if !registered_secret.is_empty() {
        let presented = req.client_secret.as_deref().unwrap_or("");
        // Constant-time compare (bytewise XOR fold).
        let ok = registered_secret.len() == presented.len()
            && registered_secret
                .as_bytes()
                .iter()
                .zip(presented.as_bytes().iter())
                .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                == 0;
        if !ok {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "invalid_client"})),
            )
                .into_response();
        }
    }

    let access_token = random_hex(32);
    let refresh_token = random_hex(32);
    let expires = now_secs() + ACCESS_TOKEN_TTL_SECS;

    let at_key = format!("oidc:token:{access_token}");
    let rt_key = format!("oidc:refresh:{refresh_token}");
    let del_code_key = format!("oidc:code:{code}");
    let user_c = user.clone();
    let scope_c = scope.clone();
    let client_c = client_id.clone();
    let _ = with_kevy(move |c| {
        c.hset(
            at_key.as_bytes(),
            &[
                (b"user" as &[u8], user_c.as_bytes()),
                (b"client_id", client_c.as_bytes()),
                (b"scope", scope_c.as_bytes()),
                (b"expires_at", expires.to_string().as_bytes()),
            ],
        )?;
        c.hset(
            rt_key.as_bytes(),
            &[
                (b"user" as &[u8], user_c.as_bytes()),
                (b"client_id", client_c.as_bytes()),
                (b"scope", scope_c.as_bytes()),
            ],
        )?;
        c.del(&[del_code_key.as_bytes()])?;
        Ok(())
    });

    let id_token = build_id_token_opaque(&user, &client_id, expires);
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "access_token": access_token,
            "token_type": "Bearer",
            "expires_in": ACCESS_TOKEN_TTL_SECS,
            "refresh_token": refresh_token,
            "scope": scope,
            "id_token": id_token,
        })),
    )
        .into_response()
}

async fn refresh_access_token(req: TokenRequest) -> axum::response::Response {
    let Some(refresh) = req.refresh_token else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid_request"})),
        )
            .into_response();
    };
    let rt_key = format!("oidc:refresh:{refresh}");
    let flat = with_kevy(move |c| c.hgetall(rt_key.as_bytes()).map_err(std::io::Error::from))
        .unwrap_or_default();
    if flat.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid_grant"})),
        )
            .into_response();
    }
    let mut fields = std::collections::HashMap::new();
    let mut i = 0;
    while i + 1 < flat.len() {
        fields.insert(
            String::from_utf8_lossy(&flat[i]).to_string(),
            String::from_utf8_lossy(&flat[i + 1]).to_string(),
        );
        i += 2;
    }
    let access_token = random_hex(32);
    let user = fields.get("user").cloned().unwrap_or_default();
    let client_id = fields.get("client_id").cloned().unwrap_or_default();
    let scope = fields.get("scope").cloned().unwrap_or_default();
    let expires = now_secs() + ACCESS_TOKEN_TTL_SECS;
    let at_key = format!("oidc:token:{access_token}");
    let user_c = user.clone();
    let scope_c = scope.clone();
    let client_c = client_id.clone();
    let _ = with_kevy(move |c| {
        c.hset(
            at_key.as_bytes(),
            &[
                (b"user" as &[u8], user_c.as_bytes()),
                (b"client_id", client_c.as_bytes()),
                (b"scope", scope_c.as_bytes()),
                (b"expires_at", expires.to_string().as_bytes()),
            ],
        )?;
        Ok(())
    });
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "access_token": access_token,
            "token_type": "Bearer",
            "expires_in": ACCESS_TOKEN_TTL_SECS,
            "scope": scope,
        })),
    )
        .into_response()
}

/// GET /oauth/userinfo — introspection endpoint. Reads the Bearer,
/// returns the subject profile.
pub async fn userinfo(headers: axum::http::HeaderMap) -> impl IntoResponse {
    let Some(auth) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "no_token"})),
        )
            .into_response();
    };
    let Some(token) = auth.strip_prefix("Bearer ").map(str::trim) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid_auth_scheme"})),
        )
            .into_response();
    };
    let key = format!("oidc:token:{token}");
    let flat = with_kevy(move |c| c.hgetall(key.as_bytes()).map_err(std::io::Error::from))
        .unwrap_or_default();
    if flat.is_empty() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid_token"})),
        )
            .into_response();
    }
    let mut fields = std::collections::HashMap::new();
    let mut i = 0;
    while i + 1 < flat.len() {
        fields.insert(
            String::from_utf8_lossy(&flat[i]).to_string(),
            String::from_utf8_lossy(&flat[i + 1]).to_string(),
        );
        i += 2;
    }
    let user = fields.get("user").cloned().unwrap_or_default();
    let expires_at: i64 = fields
        .get("expires_at")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    if expires_at < now_secs() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "token_expired"})),
        )
            .into_response();
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "sub": user,
            "email": user,
            "email_verified": true,
            "preferred_username": user.split('@').next().unwrap_or(&user),
        })),
    )
        .into_response()
}

// ── legacy /api/auth/oidc/{login,callback} for external IdP mode ──
