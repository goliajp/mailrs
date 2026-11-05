//! Restoring a deleted account before its purge, as fastcore does: here
//! the account row never left, so restoring clears its `deleted_at`, and
//! every row that references it is there with it.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use mailrs_core_api::method::admin::{RestoreAccountRequest, RestoreRefusal, RestoreRefused};
use mailrs_core_sidestate::NetKevy;
use mailrs_core_sidestate::families::account_purge::restore;

use super::account_purge::is_account;
use crate::core_rpc::CoreRpcState;

fn failed(what: &'static str) -> impl Fn(std::io::Error) -> StatusCode {
    move |e| {
        tracing::error!(error = %e, "restoring the account: {what} failed");
        StatusCode::INTERNAL_SERVER_ERROR
    }
}

/// POST /v1/admin/accounts/{address}/restore
pub async fn restore_account(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
    Json(req): Json<RestoreAccountRequest>,
) -> Result<axum::response::Response, StatusCode> {
    let live = is_account(&state, &address).await?;
    let st = state.clone();
    let (a, parked) = (address.clone(), req.parked.clone());
    let checked = tokio::task::spawn_blocking(move || {
        let mut c = st.net_conn().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
        let root = std::path::Path::new(&st.maildir_root);
        restore::check(&mut c, root, &a, live, parked.as_deref()).map_err(failed("checking"))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;
    let dir = match checked {
        Ok(dir) => dir,
        Err(r) => return Ok((StatusCode::CONFLICT, Json(r)).into_response()),
    };
    let restored = state.domain.restore_account(&address).await.map_err(|e| {
        tracing::error!(error = %e, %address, "restoring the account row failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    if !restored {
        let refused = RestoreRefused {
            refused: RestoreRefusal::NoRecord,
            parked: Vec::new(),
        };
        return Ok((StatusCode::CONFLICT, Json(refused)).into_response());
    }
    let st = state.clone();
    let a = address.clone();
    tokio::task::spawn_blocking(move || {
        let mut c = st.net_conn().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
        let root = std::path::Path::new(&st.maildir_root);
        let now = chrono::Utc::now().timestamp();
        restore::finish(&mut c, root, &a, dir.as_deref(), &req.by, now)
            .map_err(failed("moving the maildir back"))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;
    tracing::info!(%address, "deleted account restored");
    Ok(StatusCode::NO_CONTENT.into_response())
}
