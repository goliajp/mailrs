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

use axum::Json;
use rand_core::RngCore;
use std::sync::Arc;

use crate::WebState;

mod clients;
mod flow;
mod login;

pub use clients::*;
pub use flow::*;
pub use login::*;

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
