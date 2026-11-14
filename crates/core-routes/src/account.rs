//! Making accounts and changing their settings.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_core_api::method::admin as adm;
use mailrs_core_sidestate::families::{account_purge::pending, audit, quota_limits};

use crate::store::{AccountPatch, AccountStore, NewAccount, StoreError};

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn failed(e: StoreError, address: &str) -> Response {
    tracing::error!(error = %e, %address, "account store failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// Run `f` on the shared store. Without one configured (tests, a degraded
/// deploy) nothing runs; one configured but unreachable is a 503, so no
/// account is made without a limit.
fn on_net<S: AccountStore, T>(
    state: &S,
    f: impl FnOnce(&mut kevy_client::Connection) -> std::io::Result<T>,
) -> Result<Option<T>, StatusCode> {
    if !state.net_configured() {
        return Ok(None);
    }
    let mut c = state.net_conn().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    f(&mut c).map(Some).map_err(|e| {
        tracing::warn!(error = %e, "shared store failed");
        StatusCode::SERVICE_UNAVAILABLE
    })
}

/// GET /v1/admin/accounts/{address}/hash — for the sign-in paths only.
pub async fn account_with_hash<S: AccountStore>(
    State(state): State<Arc<S>>,
    Path(address): Path<String>,
) -> Response {
    match state.account(&address).await {
        Ok(Some(a)) => Json(a).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => failed(e, &address),
    }
}

/// GET /v1/admin/accounts
pub async fn list_accounts<S: AccountStore>(State(state): State<Arc<S>>) -> Response {
    match state.accounts().await {
        Ok(items) => Json(adm::AccountListResponse { items }).into_response(),
        Err(e) => failed(e, ""),
    }
}

fn hash_of(initial: adm::InitialPassword<'_>) -> Option<String> {
    use argon2::{
        Argon2, PasswordHasher,
        password_hash::{SaltString, rand_core::OsRng},
    };
    match initial {
        adm::InitialPassword::Hash(h) => Some(h.to_string()),
        adm::InitialPassword::Plain(plain) => {
            let salt = SaltString::generate(&mut OsRng);
            Argon2::default()
                .hash_password(plain.as_bytes(), &salt)
                .ok()
                .map(|h| h.to_string())
        }
    }
}

/// POST /v1/admin/accounts — a new account. An address that has one is a
/// 409: creating over it would reset its quota and recovery email, and
/// passwords have their own route. So is one whose deleted account waits
/// for its purge, which an account made now would be handed.
pub async fn add_account<S: AccountStore>(
    State(state): State<Arc<S>>,
    Query(by): Query<adm::ActorQuery>,
    Json(req): Json<adm::AddAccountRequest>,
) -> Response {
    let Some(initial) = req.initial_password() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match state.account(&req.address).await {
        Ok(None) => {}
        Ok(Some(_)) => return StatusCode::CONFLICT.into_response(),
        Err(e) => return failed(e, &req.address),
    }
    match on_net(&*state, |c| pending::deleted_at(c, &req.address)) {
        Ok(Some(Some(_))) => {
            tracing::warn!(address = %req.address, "address pending purge; refused");
            return StatusCode::CONFLICT.into_response();
        }
        Ok(_) => {}
        Err(code) => return code.into_response(),
    }
    let Some(hash) = hash_of(initial) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    // the receiver reads the limit from the shared store
    if let Err(code) = on_net(&*state, |c| quota_limits::give_default(c, &req.address)) {
        return code.into_response();
    }
    let domain = req.address.rsplit_once('@').map(|(_, d)| d).unwrap_or("");
    let at = now();
    let new = NewAccount {
        address: &req.address,
        domain,
        display_name: &req.display_name,
        password_hash: &hash,
        created_at: at,
        quota_bytes: adm::DEFAULT_QUOTA_BYTES,
    };
    if let Err(e) = state.insert_account(&new).await {
        return failed(e, &req.address);
    }
    audit::record_by_core(
        &*state,
        by.actor(),
        audit::ACCOUNT_CREATE,
        &req.address,
        "",
        at,
    );
    StatusCode::NO_CONTENT.into_response()
}

async fn patch<S: AccountStore>(state: &S, address: &str, p: AccountPatch<'_>) -> Response {
    match state.patch_account(address, p).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => failed(e, address),
    }
}

/// PUT /v1/admin/accounts/{address} — the display name.
pub async fn update_account<S: AccountStore>(
    State(state): State<Arc<S>>,
    Path(address): Path<String>,
    Json(req): Json<adm::UpdateAccountRequest>,
) -> Response {
    patch(
        &*state,
        &address,
        AccountPatch::DisplayName(&req.display_name),
    )
    .await
}

/// POST /v1/admin/accounts/{address}/quota — the limit the receiver
/// enforces, in the shared store, and the figure the account shows.
pub async fn set_quota<S: AccountStore>(
    State(state): State<Arc<S>>,
    Path(address): Path<String>,
    Json(req): Json<adm::SetQuotaRequest>,
) -> Response {
    if let Err(code) = on_net(&*state, |c| {
        quota_limits::set_limit(c, &address, req.quota_bytes)
    }) {
        return code.into_response();
    }
    patch(&*state, &address, AccountPatch::Quota(req.quota_bytes)).await
}

/// POST /v1/admin/accounts/{address}/recovery-email
pub async fn set_recovery_email<S: AccountStore>(
    State(state): State<Arc<S>>,
    Path(address): Path<String>,
    Json(req): Json<adm::UpdateRecoveryEmailRequest>,
) -> Response {
    patch(
        &*state,
        &address,
        AccountPatch::RecoveryEmail(&req.recovery_email),
    )
    .await
}

/// POST /v1/admin/accounts/{address}/password — the hash only. Called
/// directly on the host it signs no session out and revokes no agent key,
/// and the answer says so; after a compromise use webapi's reset instead.
pub async fn set_password<S: AccountStore>(
    State(state): State<Arc<S>>,
    Path(address): Path<String>,
    Json(req): Json<adm::SetPasswordRequest>,
) -> Response {
    let done = patch(
        &*state,
        &address,
        AccountPatch::PasswordHash(&req.password_hash),
    )
    .await;
    if done.status() != StatusCode::NO_CONTENT {
        return done;
    }
    Json(adm::SetPasswordResponse::NOTHING_REVOKED).into_response()
}

/// POST /v1/admin/accounts/{address}/password:clear — recording who and
/// when; 404 for no account.
pub async fn clear_password<S: AccountStore>(
    State(state): State<Arc<S>>,
    Path(address): Path<String>,
    Json(req): Json<adm::ClearPasswordRequest>,
) -> Response {
    let p = AccountPatch::ClearPassword {
        by: &req.by,
        at: now(),
    };
    patch(&*state, &address, p).await
}
