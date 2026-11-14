//! Quota limits in the shared store, where the receiver reads them: the
//! default a new account gets, the limit an operator sets, and the pass
//! that gives the default to accounts created without one. The same logic
//! as fastcore, through `core_sidestate::families::quota_limits`.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use mailrs_core_api::method::admin as wire;
use mailrs_core_sidestate::NetKevy;
use mailrs_core_sidestate::families::{quota_limits, usage_drift};

use crate::core_rpc::CoreRpcState;

/// Run `f` on the shared store. Without a configured one (tests, a
/// degraded deploy) there is no quota enforcement to feed: nothing runs.
/// A configured store that cannot be reached is an error, so no account
/// is made without a limit.
pub(super) fn with_net<T>(
    state: &CoreRpcState,
    f: impl FnOnce(&mut kevy_client::Connection) -> std::io::Result<T>,
) -> Result<Option<T>, StatusCode> {
    if state.net_url.is_none() {
        return Ok(None);
    }
    let mut c = state.net_conn().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    f(&mut c).map(Some).map_err(|e| {
        tracing::warn!(error = %e, "shared store write failed");
        StatusCode::SERVICE_UNAVAILABLE
    })
}

#[derive(serde::Deserialize)]
pub struct QuotaLimitsQuery {
    #[serde(default = "dry_run_unless_asked")]
    dry_run: bool,
}

fn dry_run_unless_asked() -> bool {
    true
}

/// POST /v1/admin/maintenance:quota-limits
pub async fn quota_limits_route(
    State(state): State<Arc<CoreRpcState>>,
    Query(q): Query<QuotaLimitsQuery>,
) -> Result<Json<wire::QuotaLimitsResponse>, StatusCode> {
    let users: Vec<String> = state
        .domain
        .list_accounts()
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "list_accounts failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .into_iter()
        .map(|a| a.address)
        .collect();
    with_net(&state, |c| quota_limits::fill_in(c, &users, q.dry_run))?
        .map(Json)
        // no shared store: nowhere a limit could be read from or written to
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)
}

/// The usage-drift pass in the SQL core: the same pass as fastcore,
/// through `core_sidestate::families::usage_drift`.
async fn usage_drift(
    state: Arc<CoreRpcState>,
    dry_run: bool,
) -> Result<Json<wire::UsageDriftResponse>, StatusCode> {
    let users: Vec<String> = state
        .domain
        .list_accounts()
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "list_accounts failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .into_iter()
        .map(|a| a.address)
        .collect();
    let root = std::path::PathBuf::from(&state.maildir_root);
    let now = chrono::Utc::now().timestamp();
    let done = tokio::task::spawn_blocking(move || {
        with_net(&state, |c| {
            usage_drift::pass(c, &root, &users, dry_run, now)
        })
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    done?
        .map(Json)
        // no shared store: there is no counter to measure
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)
}

/// GET /v1/admin/maintenance:usage-drift — reports only.
pub async fn usage_drift_route(
    State(state): State<Arc<CoreRpcState>>,
) -> Result<Json<wire::UsageDriftResponse>, StatusCode> {
    usage_drift(state, true).await
}

#[derive(serde::Deserialize)]
pub struct UsageDriftQuery {
    #[serde(default = "dry_run_unless_asked")]
    dry_run: bool,
}

/// POST /v1/admin/maintenance:usage-drift — `?dry_run=false` corrects.
pub async fn usage_drift_apply_route(
    State(state): State<Arc<CoreRpcState>>,
    Query(q): Query<UsageDriftQuery>,
) -> Result<Json<wire::UsageDriftResponse>, StatusCode> {
    usage_drift(state, q.dry_run).await
}
