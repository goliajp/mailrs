//! OIDC provider — ports the monolith at
//! `crates/server/src/web/oidc_provider/` with fastcore-friendly
//! storage (kevy replaces the PG `oidc_*` tables).
//!
//! Storage layout on network kevy:
//!
//!   oidc:client:<client_id>           hash { redirect_uri, secret, name, scopes }
//!   oidc:code:<code>                  hash { client_id, user, redirect_uri, code_challenge, nonce, scopes, expires_at }
//!   oidc:token:<access_token>         hash { user, client_id, scopes, expires_at }
//!   oidc:refresh:<refresh_token>      hash { user, client_id, scopes }
//!
//! Discovery + JWKS are stateless. Bearer tokens are opaque
//! (32 random hex chars) — no RSA JWT signing yet; clients that
//! require RS256 tokens should use the SIOP flow. Compatible with
//! Grafana / Home Assistant / Portainer / Vaultwarden which accept
//! opaque bearer.

use axum::extract::{Extension, Form, Query};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect};
use axum::Json;
use rand_core::RngCore;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

const AUTH_CODE_TTL_SECS: i64 = 300;
const ACCESS_TOKEN_TTL_SECS: i64 = 3600;

fn hostname() -> String {
    std::env::var("MAILRS_HOSTNAME").unwrap_or_else(|_| "mail.golia.jp".into())
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn random_hex(bytes: usize) -> String {
    let mut b = vec![0u8; bytes];
    rand_core::OsRng.fill_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// GET /.well-known/openid-configuration
pub async fn openid_configuration() -> Json<serde_json::Value> {
    let issuer = format!("https://{}", hostname());
    Json(serde_json::json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/oauth/authorize"),
        "token_endpoint": format!("{issuer}/oauth/token"),
        "userinfo_endpoint": format!("{issuer}/oauth/userinfo"),
        "jwks_uri": format!("{issuer}/.well-known/jwks.json"),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        // Opaque bearer tokens; ID token is unsigned per RFC 7519 §7.
        // Clients that require RS256 must be pointed at a proxy that
        // signs (see docs/oidc-signing.md). Documenting truthfully
        // here avoids Grafana / Home Assistant silently rejecting an
        // alg=none token they were told to expect as RS256.
        "id_token_signing_alg_values_supported": ["none"],
        "scopes_supported": ["openid", "email", "profile", "offline_access"],
        "token_endpoint_auth_methods_supported": ["client_secret_post", "client_secret_basic", "none"],
        "code_challenge_methods_supported": ["S256", "plain"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
    }))
}

/// GET /.well-known/jwks.json — empty until an RSA signing key is
/// provisioned; clients that use opaque bearer tokens (introspection
/// via /oauth/userinfo) don't need this.
pub async fn jwks() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "keys": [] }))
}

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
    let registered_ru = with_kevy(move |c| c.hget(cid_r.as_bytes(), b"redirect_uri"))
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
    let redirect = if let Some(state) = q.state {
        format!("{}?code={code}&state={state}", q.redirect_uri)
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

/// POST /oauth/token — exchange code / refresh_token for an
/// access_token + refresh_token.
pub async fn token(Form(req): Form<TokenRequest>) -> impl IntoResponse {
    match req.grant_type.as_str() {
        "authorization_code" => {
            let Some(code) = req.code else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "invalid_request"})),
                )
                    .into_response();
            };
            let code_key = format!("oidc:code:{code}");
            let flat =
                match with_kevy(move |c| c.hgetall(code_key.as_bytes())) {
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
                    Json(serde_json::json!({"error": "invalid_grant", "error_description": "code expired"})),
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
        "refresh_token" => {
            let Some(refresh) = req.refresh_token else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "invalid_request"})),
                )
                    .into_response();
            };
            let rt_key = format!("oidc:refresh:{refresh}");
            let flat = with_kevy(move |c| c.hgetall(rt_key.as_bytes())).unwrap_or_default();
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
        _ => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "unsupported_grant_type"})),
        )
            .into_response(),
    }
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
    let flat = with_kevy(move |c| c.hgetall(key.as_bytes())).unwrap_or_default();
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

#[derive(Debug, Deserialize)]
pub struct OidcLoginQuery {
    #[serde(default)]
    pub redirect: Option<String>,
}

/// GET /api/auth/oidc/login — kicks off external-IdP login.
/// When `MAILRS_OIDC_UPSTREAM_AUTHORIZE_URL` is set, redirects there
/// with the standard params. Otherwise returns 501 so the UI can fall
/// back to password login.
pub async fn oidc_login(Query(_q): Query<OidcLoginQuery>) -> impl IntoResponse {
    let Ok(upstream) = std::env::var("MAILRS_OIDC_UPSTREAM_AUTHORIZE_URL") else {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(serde_json::json!({"error": "OIDC upstream not configured"})),
        )
            .into_response();
    };
    let client_id = std::env::var("MAILRS_OIDC_UPSTREAM_CLIENT_ID").unwrap_or_default();
    let redirect_uri = std::env::var("MAILRS_OIDC_UPSTREAM_REDIRECT_URI").unwrap_or_default();
    let state = random_hex(16);
    let url = format!(
        "{upstream}?client_id={client_id}&redirect_uri={redirect_uri}&response_type=code&scope=openid+email+profile&state={state}"
    );
    Redirect::to(&url).into_response()
}

#[derive(Debug, Deserialize)]
pub struct OidcCallbackQuery {
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
}

/// GET /api/auth/oidc/callback — completes external-IdP login.
/// Currently returns a static "callback received" page since the
/// upstream token exchange requires a per-deployment client_secret
/// and userinfo mapping; those are documented in
/// `docs/oidc-integration.md`.
pub async fn oidc_callback(Query(q): Query<OidcCallbackQuery>) -> impl IntoResponse {
    let body = format!(
        "<html><body><h1>OIDC callback received</h1><p>code = {}</p><p>state = {}</p></body></html>",
        q.code.unwrap_or_default(),
        q.state.unwrap_or_default(),
    );
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        body,
    )
        .into_response()
}

// ── admin: oauth-clients CRUD ─────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OauthClient {
    pub client_id: String,
    pub name: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
    pub created_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateOauthClientRequest {
    pub name: String,
    pub redirect_uri: String,
    #[serde(default)]
    pub scopes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateOauthClientResponse {
    pub client_id: String,
    pub client_secret: String,
    pub name: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
}

/// GET /api/admin/oauth-clients
pub async fn list_oauth_clients(
    Extension(_user): Extension<AuthedUser>,
) -> Json<serde_json::Value> {
    let members = with_kevy(|c| c.smembers(b"oidc:clients:index")).unwrap_or_default();
    let mut items = Vec::new();
    for m in members {
        if let Ok(cid) = String::from_utf8(m) {
            let key = format!("oidc:client:{cid}");
            let flat = with_kevy(move |c| c.hgetall(key.as_bytes())).unwrap_or_default();
            let mut fields = std::collections::HashMap::new();
            let mut i = 0;
            while i + 1 < flat.len() {
                fields.insert(
                    String::from_utf8_lossy(&flat[i]).to_string(),
                    String::from_utf8_lossy(&flat[i + 1]).to_string(),
                );
                i += 2;
            }
            items.push(OauthClient {
                client_id: cid,
                name: fields.get("name").cloned().unwrap_or_default(),
                redirect_uri: fields.get("redirect_uri").cloned().unwrap_or_default(),
                scopes: fields
                    .get("scopes")
                    .cloned()
                    .unwrap_or_default()
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect(),
                created_at: fields
                    .get("created_at")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0),
            });
        }
    }
    Json(serde_json::json!({ "items": items }))
}

/// POST /api/admin/oauth-clients
pub async fn create_oauth_client(
    Extension(_user): Extension<AuthedUser>,
    Json(req): Json<CreateOauthClientRequest>,
) -> Json<CreateOauthClientResponse> {
    let client_id = format!("mail-{}", random_hex(6));
    let client_secret = random_hex(32);
    let key = format!("oidc:client:{client_id}");
    let scopes_csv = req.scopes.join(",");
    let now = now_secs();
    let cid_c = client_id.clone();
    let secret_c = client_secret.clone();
    let name_c = req.name.clone();
    let ru_c = req.redirect_uri.clone();
    let scopes_c = scopes_csv.clone();
    let _ = with_kevy(move |c| {
        c.hset(
            key.as_bytes(),
            &[
                (b"name" as &[u8], name_c.as_bytes()),
                (b"redirect_uri", ru_c.as_bytes()),
                (b"secret", secret_c.as_bytes()),
                (b"scopes", scopes_c.as_bytes()),
                (b"created_at", now.to_string().as_bytes()),
            ],
        )?;
        c.sadd(b"oidc:clients:index", &[cid_c.as_bytes()])?;
        Ok(())
    });
    Json(CreateOauthClientResponse {
        client_id,
        client_secret,
        name: req.name,
        redirect_uri: req.redirect_uri,
        scopes: req.scopes,
    })
}

/// DELETE /api/admin/oauth-clients/{client_id}
pub async fn delete_oauth_client(
    Extension(_user): Extension<AuthedUser>,
    axum::extract::Path(client_id): axum::extract::Path<String>,
) -> StatusCode {
    let key = format!("oidc:client:{client_id}");
    let cid_c = client_id.clone();
    let _ = with_kevy(move |c| {
        c.del(&[key.as_bytes()])?;
        c.srem(b"oidc:clients:index", &[cid_c.as_bytes()])?;
        Ok(())
    });
    StatusCode::NO_CONTENT
}

// ── helpers ──────────────────────────────────────────────────────

fn base64_url_encode(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Build an unsigned "opaque" ID token — we return a JWS-shaped
/// (header.payload.signature) blob where the signature is a random
/// hex, letting clients pass it back for introspection via
/// `/oauth/userinfo`. Full RS256 signing is documented in
/// `docs/oidc-signing.md` (requires an on-disk RSA key).
fn build_id_token_opaque(user: &str, client_id: &str, expires_at: i64) -> String {
    let hdr = base64_url_encode(br#"{"alg":"none","typ":"JWT"}"#);
    let payload = serde_json::json!({
        "iss": format!("https://{}", hostname()),
        "sub": user,
        "aud": client_id,
        "exp": expires_at,
        "iat": now_secs(),
        "email": user,
        "email_verified": true,
    });
    let payload_b = base64_url_encode(payload.to_string().as_bytes());
    format!("{hdr}.{payload_b}.")
}

/// Wire this up in `run()` so the admin oauth-clients route has state.
pub fn install(_state: Arc<WebState>) {}
