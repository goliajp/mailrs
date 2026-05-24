//! OIDC external IdP login (login / callback / client config).

#![allow(unused_imports)]

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Json, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};

use crate::inbound::auth_guard::AuthCheck;

use super::super::{AuthUser, SessionInfo, WebState};
use super::*;

/// initiate OIDC login — redirects user to the configured OIDC provider
pub(crate) async fn oidc_login(
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref oidc) = state.oidc_config else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "OIDC not configured"})),
        ).into_response();
    };

    let nonce: String = {
        let mut bytes = [0u8; 16];
        rand_core::OsRng.fill_bytes(&mut bytes);
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    };

    let url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope=openid+email+profile&state={}",
        oidc.authorize_url,
        urlencoding::encode(&oidc.client_id),
        urlencoding::encode(&oidc.redirect_uri),
        nonce,
    );

    axum::response::Redirect::temporary(&url).into_response()
}

/// OIDC callback — exchange code for token, match to local account, create session
pub(crate) async fn oidc_callback(
    axum::extract::Query(q): axum::extract::Query<OidcCallbackQuery>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref oidc) = state.oidc_config else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "OIDC not configured"})),
        ).into_response();
    };

    // exchange code for token
    let client = reqwest::Client::new();
    let token_res = client
        .post(&oidc.token_url)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", &q.code),
            ("redirect_uri", &oidc.redirect_uri),
            ("client_id", &oidc.client_id),
            ("client_secret", &oidc.client_secret),
        ])
        .send()
        .await;

    let token_body: serde_json::Value = match token_res {
        Ok(res) if res.status().is_success() => match res.json().await {
            Ok(v) => v,
            Err(_) => {
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({"error": "invalid token response"})),
                ).into_response();
            }
        },
        _ => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": "token exchange failed"})),
            ).into_response();
        }
    };

    let access_token = token_body["access_token"].as_str().unwrap_or_default();
    if access_token.is_empty() {
        return (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": "no access_token in response"})),
        ).into_response();
    }

    // fetch userinfo
    let userinfo_res = client
        .get(&oidc.userinfo_url)
        .bearer_auth(access_token)
        .send()
        .await;

    let userinfo: serde_json::Value = match userinfo_res {
        Ok(res) if res.status().is_success() => match res.json().await {
            Ok(v) => v,
            Err(_) => {
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({"error": "invalid userinfo response"})),
                ).into_response();
            }
        },
        _ => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": "userinfo fetch failed"})),
            ).into_response();
        }
    };

    // extract email — this must match a local account
    let email = userinfo["email"].as_str().or_else(|| userinfo["sub"].as_str()).unwrap_or_default();
    if email.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "no email in userinfo"})),
        ).into_response();
    }

    // look up local account
    let Some(ref ds) = state.domain_store else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "auth not configured"})),
        ).into_response();
    };

    let account = match ds.get_account_with_hash(email).await {
        Ok(Some((a, _hash))) if a.active => a,
        _ => {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"error": format!("no mailrs account for {email}")})),
            ).into_response();
        }
    };

    // create session (same as login, but skip password/TOTP — already verified by IdP)
    let permissions = Arc::new(
        ds.load_account_permissions(&account.address)
            .await
            .unwrap_or_else(|_| crate::permission::compute_effective_permissions(&[], &[], &[])),
    );

    let mut bytes = [0u8; 32];
    rand_core::OsRng.fill_bytes(&mut bytes);
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();

    state.sessions.insert(
        token.clone(),
        SessionInfo {
            address: account.address.clone(),
            display_name: account.display_name.clone(),
            permissions: permissions.clone(),
            created_at: std::time::Instant::now(),
        },
    );

    ds.log_audit(&account.address, "oidc_login", &oidc.client_id, "").await;

    // redirect to frontend with token as query param (frontend stores it)
    let redirect_url = format!("/login?oidc_token={}&address={}&display_name={}",
        urlencoding::encode(&token),
        urlencoding::encode(&account.address),
        urlencoding::encode(&account.display_name),
    );
    axum::response::Redirect::temporary(&redirect_url).into_response()
}

/// returns OIDC availability info for the frontend login page (no auth required)
pub(crate) async fn oidc_client_config(
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    match &state.oidc_config {
        Some(_) => Json(serde_json::json!({
            "enabled": true,
            "login_url": "/api/auth/oidc/login",
            "provider_name": "GOLIA",
        })),
        None => Json(serde_json::json!({
            "enabled": false,
        })),
    }
}
