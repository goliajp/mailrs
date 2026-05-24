//! Password lifecycle: forgot, reset, change, recovery email.

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

pub(crate) async fn forgot_password(
    State(state): State<Arc<WebState>>,
    Json(req): Json<ForgotPasswordRequest>,
) -> impl IntoResponse {
    if req.address.is_empty() || req.address.len() > crate::web::MAX_ADMIN_FIELD_LEN {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid address"})),
        );
    }
    if req.recovery_email.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "recovery email is required"})),
        );
    }

    let Some(ref pool) = state.pg_pool else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "auth backend unavailable"})),
        );
    };

    // look up account and verify recovery email matches
    let recovery_match = if let Some(ref ds) = state.domain_store {
        ds.get_account_with_hash(&req.address)
            .await
            .ok()
            .flatten()
            .map(|(acct, _)| {
                !acct.recovery_email.is_empty()
                    && acct.recovery_email.eq_ignore_ascii_case(&req.recovery_email)
            })
            .unwrap_or(false)
    } else {
        false
    };

    if recovery_match {
        let recovery_email = req.recovery_email.clone();
        {
            // generate reset token
            let mut bytes = [0u8; 32];
            rand_core::OsRng.fill_bytes(&mut bytes);
            let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();

            let expires_at = Utc::now() + chrono::Duration::hours(1);

            let inserted = sqlx::query(
                "INSERT INTO password_reset_tokens (account_address, token, expires_at) \
                 VALUES ($1, $2, $3)",
            )
            .bind(&req.address)
            .bind(&token)
            .bind(expires_at)
            .execute(pool)
            .await;

            if inserted.is_ok() {
                // send reset email to the RECOVERY email via outbound queue
                let reset_link = format!("https://mail.golia.ai/reset-password?token={token}");
                let subject = "Password Reset Request";
                let text_body = format!(
                    "You requested a password reset for {address}.\n\n\
                     Click the link below to reset your password:\n\
                     {reset_link}\n\n\
                     This link expires in 1 hour.\n\n\
                     If you did not request this, please ignore this email.",
                    address = req.address,
                );
                let html_body = format!(
                    "<p>You requested a password reset for <strong>{address}</strong>.</p>\
                     <p>Click the link below to reset your password:</p>\
                     <p><a href=\"{reset_link}\">Reset Password</a></p>\
                     <p>This link expires in 1 hour.</p>\
                     <p>If you did not request this, please ignore this email.</p>",
                    address = req.address,
                );

                let now = Utc::now();
                let message_id = format!(
                    "{}.{}@{}",
                    now.timestamp_millis(),
                    rand_core::OsRng.next_u32(),
                    state.hostname
                );
                let from = format!("noreply@{}", state.hostname);
                let to = vec![recovery_email];
                let raw = crate::web::mail::build_rfc5322_message(
                    &from,
                    &to,
                    &[],
                    subject,
                    &text_body,
                    Some(&html_body),
                    &message_id,
                    None,
                    &[],
                    &now,
                    None,
                );

                // send via outbound queue so it reaches the external recovery email
                if let Some(ref oq) = state.outbound_queue {
                    let rcpt = &to[0];
                    let domain = rcpt
                        .rsplit_once('@')
                        .map(|(_, d)| d)
                        .unwrap_or("unknown");
                    let ts = now.timestamp();
                    let _ = mailrs_outbound_queue::queue::enqueue(
                        oq,
                        &from,
                        rcpt,
                        domain,
                        &raw,
                        Some(&message_id),
                        ts,
                    )
                    .await;
                }

                if let Some(ref ds) = state.domain_store {
                    ds.log_audit(&req.address, "password_reset_requested", &req.address, "")
                        .await;
                }
            }
        }
    }

    // always return same response to prevent enumeration
    (
        StatusCode::OK,
        Json(serde_json::json!({"success": true, "message": "If the account and recovery email match, a reset link has been sent."})),
    )
}

pub(crate) async fn reset_password(
    State(state): State<Arc<WebState>>,
    Json(req): Json<ResetPasswordRequest>,
) -> impl IntoResponse {
    if req.token.is_empty() || req.token.len() > 128 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid token"})),
        );
    }

    if let Err(e) = crate::users::validate_password(&req.new_password) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        );
    }

    let Some(ref pool) = state.pg_pool else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "auth backend unavailable"})),
        );
    };

    // look up token
    let row: Option<(i64, String, bool)> = sqlx::query_as(
        "SELECT id, account_address, used FROM password_reset_tokens \
         WHERE token = $1 AND expires_at > now()",
    )
    .bind(&req.token)
    .fetch_optional(pool)
    .await
    .unwrap_or(None);

    let Some((token_id, account_address, used)) = row else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid or expired token"})),
        );
    };

    if used {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "token already used"})),
        );
    }

    // hash new password
    let password_hash = match crate::users::UserStore::hash_password(&req.new_password) {
        Ok(h) => h,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "failed to hash password"})),
            );
        }
    };

    // update password via domain store
    let Some(ref ds) = state.domain_store else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "auth backend unavailable"})),
        );
    };

    // get current account to preserve domain and display_name
    let (account, _) = match ds.get_account_with_hash(&account_address).await {
        Ok(Some(pair)) => pair,
        _ => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "account not found"})),
            );
        }
    };

    let now = Utc::now().timestamp();
    if let Err(e) = ds
        .add_account(
            &account.address,
            &account.domain,
            &account.display_name,
            &password_hash,
            now,
        )
        .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("failed to update password: {e}")})),
        );
    }

    // mark token as used
    let _ = sqlx::query("UPDATE password_reset_tokens SET used = true WHERE id = $1")
        .bind(token_id)
        .execute(pool)
        .await;

    // audit log
    ds.log_audit(&account_address, "password_reset", &account_address, "")
        .await;

    (
        StatusCode::OK,
        Json(serde_json::json!({"success": true})),
    )
}

pub(crate) async fn change_password(
    State(state): State<Arc<WebState>>,
    AuthUser { address, .. }: AuthUser,
    Json(req): Json<ChangePasswordRequest>,
) -> impl IntoResponse {
    if req.current_password.is_empty() || req.new_password.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "current and new password are required"})),
        );
    }

    if let Err(e) = crate::users::validate_password(&req.new_password) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        );
    }

    let Some(ref ds) = state.domain_store else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "auth backend unavailable"})),
        );
    };

    // verify current password
    let (account, password_hash) = match ds.get_account_with_hash(&address).await {
        Ok(Some(pair)) => pair,
        _ => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "account not found"})),
            );
        }
    };

    let valid = if password_hash.is_empty() {
        false
    } else if password_hash.starts_with("$argon2") {
        crate::users::UserStore::verify_hash(&req.current_password, &password_hash)
    } else {
        password_hash == req.current_password
    };

    if !valid {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "current password is incorrect"})),
        );
    }

    // hash new password
    let new_hash = match crate::users::UserStore::hash_password(&req.new_password) {
        Ok(h) => h,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "failed to hash password"})),
            );
        }
    };

    let now = Utc::now().timestamp();
    if let Err(e) = ds
        .add_account(
            &account.address,
            &account.domain,
            &account.display_name,
            &new_hash,
            now,
        )
        .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("failed to update password: {e}")})),
        );
    }

    ds.log_audit(&address, "password_changed", &address, "").await;

    (
        StatusCode::OK,
        Json(serde_json::json!({"success": true})),
    )
}

pub(crate) async fn update_recovery_email(
    State(state): State<Arc<WebState>>,
    AuthUser { address, .. }: AuthUser,
    Json(req): Json<UpdateRecoveryEmailRequest>,
) -> impl IntoResponse {
    if req.recovery_email.len() > crate::web::MAX_ADMIN_FIELD_LEN {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid email length"})),
        );
    }

    // basic email format validation (if not empty)
    if !req.recovery_email.is_empty() && !req.recovery_email.contains('@') {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid email format"})),
        );
    }

    let Some(ref ds) = state.domain_store else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "auth backend unavailable"})),
        );
    };

    match ds.update_recovery_email(&address, &req.recovery_email).await {
        Ok(true) => {
            ds.log_audit(&address, "recovery_email_updated", &address, "").await;
            (
                StatusCode::OK,
                Json(serde_json::json!({"success": true})),
            )
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "account not found"})),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("failed to update recovery email: {e}")})),
        ),
    }
}

pub(crate) async fn get_recovery_email(
    State(state): State<Arc<WebState>>,
    AuthUser { address, .. }: AuthUser,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(serde_json::json!({"recovery_email": ""}));
    };

    let recovery_email = match ds.get_account_with_hash(&address).await {
        Ok(Some((acct, _))) => acct.recovery_email,
        _ => String::new(),
    };

    Json(serde_json::json!({"recovery_email": recovery_email}))
}
