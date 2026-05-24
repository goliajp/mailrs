//! TOTP 2FA setup / enable / disable / status.

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

/// set up TOTP: generate secret + recovery codes, save to DB (not yet enabled)
pub(crate) async fn totp_setup(
    State(state): State<Arc<WebState>>,
    AuthUser { address, .. }: AuthUser,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "auth backend unavailable"})),
        );
    };

    let secret = crate::totp::generate_secret();
    let recovery_codes = crate::totp::generate_recovery_codes();
    let recovery_str = recovery_codes.join(",");

    if let Err(e) = ds.save_totp_secret(&address, &secret, &recovery_str).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("failed to save TOTP secret: {e}")})),
        );
    }

    let otpauth_url = crate::totp::get_otpauth_url(&secret, &address, "mailrs");

    ds.log_audit(&address, "totp_setup", "", "").await;

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "secret": secret,
            "otpauth_url": otpauth_url,
            "recovery_codes": recovery_codes,
        })),
    )
}

/// enable TOTP after verifying the user can produce a valid code
pub(crate) async fn totp_enable(
    State(state): State<Arc<WebState>>,
    AuthUser { address, .. }: AuthUser,
    Json(req): Json<TotpCodeRequest>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "auth backend unavailable"})),
        );
    };

    // get the saved (not yet enabled) secret
    let secret = match ds.get_totp_secret(&address).await {
        Ok(Some((secret, false, _))) => secret,
        Ok(Some((_, true, _))) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "TOTP already enabled"})),
            );
        }
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "TOTP not set up, call /api/auth/totp/setup first"})),
            );
        }
    };

    if !crate::totp::verify_code(&secret, &req.code) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid TOTP code"})),
        );
    }

    if let Err(e) = ds.enable_totp(&address).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("failed to enable TOTP: {e}")})),
        );
    }

    ds.log_audit(&address, "totp_enabled", "", "").await;

    (
        StatusCode::OK,
        Json(serde_json::json!({"success": true})),
    )
}

/// disable TOTP (requires a valid code to confirm)
pub(crate) async fn totp_disable(
    State(state): State<Arc<WebState>>,
    AuthUser { address, .. }: AuthUser,
    Json(req): Json<TotpCodeRequest>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "auth backend unavailable"})),
        );
    };

    // verify current TOTP code before disabling
    let secret = match ds.get_totp_secret(&address).await {
        Ok(Some((secret, true, _))) => secret,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "TOTP not enabled"})),
            );
        }
    };

    if !crate::totp::verify_code(&secret, &req.code) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid TOTP code"})),
        );
    }

    if let Err(e) = ds.disable_totp(&address).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("failed to disable TOTP: {e}")})),
        );
    }

    ds.log_audit(&address, "totp_disabled", "", "").await;

    (
        StatusCode::OK,
        Json(serde_json::json!({"success": true})),
    )
}

/// check whether TOTP is enabled for the current user
pub(crate) async fn totp_status(
    State(state): State<Arc<WebState>>,
    AuthUser { address, .. }: AuthUser,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(serde_json::json!({"enabled": false}));
    };

    let enabled = matches!(ds.get_totp_secret(&address).await, Ok(Some((_, true, _))));

    Json(serde_json::json!({"enabled": enabled}))
}
