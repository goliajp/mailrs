//! Making accounts and changing their settings.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_core_api::method::admin as adm;
use mailrs_core_sidestate::families::account_purge::{maildir, pending};
use mailrs_core_sidestate::families::{audit, quota_limits};

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
    match state.exists(&req.address).await {
        Ok(false) => {}
        Ok(true) => return StatusCode::CONFLICT.into_response(),
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

/// DELETE /v1/admin/accounts/{address} — the address stops being a
/// mailbox. Delivery takes any maildir it finds, so the maildir is moved
/// aside first; the account is kept when that fails. The rest of what the
/// account owns stays until the retention period has passed: the address
/// is marked pending purge, and the purge removes it all then, or at once
/// under a retention of 0 days.
pub async fn remove_account<S: AccountStore>(
    State(state): State<Arc<S>>,
    Path(address): Path<String>,
    Query(by): Query<adm::ActorQuery>,
) -> Response {
    let existed = match state.exists(&address).await {
        Ok(e) => e,
        Err(e) => return failed(e, &address),
    };
    let at = now();
    if !existed {
        if let Err(e) = state.drop_account(&address, at).await {
            return failed(e, &address);
        }
        return StatusCode::NO_CONTENT.into_response();
    }
    let record = match state.record_for_restore(&address).await {
        Ok(r) => r,
        Err(e) => return failed(e, &address),
    };
    let marked = on_net(&*state, |c| {
        pending::mark(c, &address, at)?;
        match &record {
            Some(r) => pending::keep_record(c, &address, r),
            None => Ok(()),
        }
    });
    let marked = match marked {
        Ok(m) => m.is_some(),
        Err(code) => {
            tracing::error!(%address, "marking the account for its purge failed; account kept");
            return code.into_response();
        }
    };
    if let Err(e) = maildir::park(state.maildir_root(), &address, at) {
        tracing::error!(error = %e, %address, "removing the account's maildir failed; account kept");
        if marked && let Err(code) = on_net(&*state, |c| pending::clear(c, &address)) {
            tracing::error!(%code, %address, "the purge mark of a kept account stays; the purge skips it while it is an account");
        }
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if let Err(e) = state.drop_account(&address, at).await {
        return failed(e, &address);
    }
    audit::record_by_core(&*state, by.actor(), audit::ACCOUNT_DELETE, &address, "", at);
    if marked && pending::retention_days() == 0 {
        state.clone().purge_now(address, at).await;
    }
    StatusCode::NO_CONTENT.into_response()
}

fn refused(r: adm::RestoreRefused) -> Response {
    (StatusCode::CONFLICT, Json(r)).into_response()
}

/// POST /v1/admin/accounts/{address}/restore — before its purge: the
/// account goes back, then its parked maildir, then the purge mark, with an
/// audit row. Everything else the account owned never left.
pub async fn restore_account<S: AccountStore>(
    State(state): State<Arc<S>>,
    Path(address): Path<String>,
    Json(req): Json<adm::RestoreAccountRequest>,
) -> Response {
    use mailrs_core_sidestate::families::account_purge::restore;
    let Some(mut net) = state.net_conn() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let live = match state.exists(&address).await {
        Ok(e) => e,
        Err(e) => return failed(e, &address),
    };
    let io_failed = |e: std::io::Error, what: &str| {
        tracing::error!(error = %e, %address, "restoring the account: {what} failed");
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    };
    let root = state.maildir_root();
    let dir = match restore::check(&mut net, root, &address, live, req.parked.as_deref()) {
        Ok(Ok(dir)) => dir,
        Ok(Err(r)) => return refused(r),
        Err(e) => return io_failed(e, "checking"),
    };
    let kept = match pending::record(&mut net, &address) {
        Ok(k) => k,
        Err(e) => return io_failed(e, "reading the kept record"),
    };
    match state.bring_back(&address, kept).await {
        Ok(true) => {}
        Ok(false) => {
            return refused(adm::RestoreRefused {
                refused: adm::RestoreRefusal::NoRecord,
                parked: Vec::new(),
            });
        }
        Err(e) => return failed(e, &address),
    }
    let back = match restore::finish(&mut net, root, &address, dir.as_deref(), &req.by, now()) {
        Ok(back) => back,
        Err(e) => return io_failed(e, "moving the maildir back"),
    };
    tracing::info!(%address, by = %req.by, rescheduled = back.rescheduled,
        drafts = back.returned_as_drafts, "deleted account restored");
    Json(back).into_response()
}
