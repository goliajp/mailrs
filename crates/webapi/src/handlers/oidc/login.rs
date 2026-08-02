//! The browser entry points that hand off to the login page.

use axum::Json;
use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect};
use serde::Deserialize;

use super::*;

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
