//! `/api/auth/*` web handlers, split by concern.

mod apikey;
use apikey::verify_api_key;
mod login;
mod oidc;
mod password;
mod totp;

pub(crate) use login::*;
pub(crate) use oidc::*;
pub(crate) use password::*;
pub(crate) use totp::*;

use std::sync::Arc;

use axum::Json;
use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use chrono::Utc;
use serde::Deserialize;

use crate::api_key_store;
use crate::permission::EffectivePermissions;

use super::{ApiResult, WebState};

#[derive(Deserialize)]
pub(crate) struct LoginRequest {
    pub address: String,
    pub password: String,
    pub totp_code: Option<String>,
}

/// how the user authenticated
#[derive(Debug, Clone)]
pub(crate) enum AuthMethod {
    Session,
    ApiKey(#[allow(dead_code)] i64),
    /// app key: (api_key_id, app_internal_id)
    AppKey(#[allow(dead_code)] i64, #[allow(dead_code)] i64),
}

/// extractor that validates bearer token and returns the authenticated user context
#[derive(Debug, Clone)]
pub(crate) struct AuthUser {
    pub address: String,
    pub display_name: String,
    pub permissions: Arc<EffectivePermissions>,
    pub auth_method: AuthMethod,
}

impl FromRequestParts<Arc<WebState>> for AuthUser {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<WebState>,
    ) -> Result<Self, Self::Rejection> {
        // try Authorization header first
        let auth_header = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let token = if let Some(t) = auth_header.strip_prefix("Bearer ") {
            Some(t.to_string())
        } else {
            // fallback: ?token= query param (for <img src>, <a href>, <iframe src>)
            parts.uri.query().and_then(|q| {
                q.split('&')
                    .find_map(|pair| pair.strip_prefix("token="))
                    .map(|t| t.to_string())
            })
        };

        if let Some(ref token) = token {
            if token.starts_with("mlrs_") {
                return verify_api_key(token, state).await;
            }

            if let Some(session) = state.sessions.get(token.as_str()) {
                let elapsed_secs =
                    crate::inbound::auth_guard::unix_now().saturating_sub(session.created_at_unix);
                if elapsed_secs < super::SESSION_TTL.as_secs() {
                    return Ok(AuthUser {
                        address: session.address.clone(),
                        display_name: session.display_name.clone(),
                        permissions: session.permissions.clone(),
                        auth_method: AuthMethod::Session,
                    });
                }
                drop(session);
                state.sessions.remove(token.as_str());
            }
        }

        Err((StatusCode::UNAUTHORIZED, "authentication required"))
    }
}

// ---------- one-purpose helpers for login_inner ----------

/// Outcome of a TOTP 2FA check during login.
enum TotpOutcome {
    /// Either TOTP isn't enabled for this account, or the
    /// provided code (or recovery code) verified successfully.
    Ok,
    /// TOTP is enabled but the request didn't include a code;
    /// caller should respond with `{"requires_totp": true}` so
    /// the client knows to prompt for one.
    RequiresCode,
    /// TOTP code (and recovery-code fallback) both rejected;
    /// caller should return the bundled 401 response.
    Failed((StatusCode, Json<serde_json::Value>)),
}

// ---- identity verification for external IdPs (login.golia.jp) ----

#[derive(Deserialize)]
pub(crate) struct VerifyRequest {
    pub address: String,
    pub password: String,
}

#[derive(Deserialize)]
pub(crate) struct VerifyTotpRequest {
    pub address: String,
    pub code: String,
}

// ---- OIDC Client (Sign in with GOLIA) ----

#[derive(Deserialize)]
pub(crate) struct OidcCallbackQuery {
    pub code: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub state: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct ForgotPasswordRequest {
    pub address: String,
    pub recovery_email: String,
}

#[derive(Deserialize)]
pub(crate) struct ResetPasswordRequest {
    pub token: String,
    pub new_password: String,
}

// --- change password (self-service) ---

#[derive(Deserialize)]
pub(crate) struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

// --- recovery email ---

#[derive(Deserialize)]
pub(crate) struct UpdateRecoveryEmailRequest {
    pub recovery_email: String,
}

// --- TOTP 2FA endpoints ---

#[derive(Deserialize)]
pub(crate) struct TotpCodeRequest {
    pub code: String,
}
