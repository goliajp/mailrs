//! How far each account's usage counter (`mailrs:quota:<user>:used_bytes`
//! in the network kevy, what the quota stage reads) has drifted from what
//! its maildir takes on disk, and the pass that corrects it. Account names
//! and numbers only: no file names, no subjects. The logic is shared with
//! the SQL core in `core_sidestate::families::usage_drift`.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::FastcoreState;

#[derive(serde::Deserialize)]
pub(crate) struct UsageDriftQuery {
    #[serde(default = "dry_run_unless_asked")]
    dry_run: bool,
}

fn dry_run_unless_asked() -> bool {
    true
}

async fn run(state: Arc<FastcoreState>, dry_run: bool) -> Response {
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::warn!(error = %e, "list_account_addresses failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let done = tokio::task::spawn_blocking(move || {
        let mut conn = state
            .net_conn()
            .ok_or_else(|| std::io::Error::other("the network store is unreachable"))?;
        mailrs_core_sidestate::families::usage_drift::pass(
            &mut conn,
            std::path::Path::new(&state.maildir_root),
            &users,
            dry_run,
            chrono::Utc::now().timestamp(),
        )
    })
    .await;
    match done {
        Ok(Ok(d)) => Json(d).into_response(),
        Ok(Err(e)) => {
            tracing::error!(error = %e, "usage drift failed");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// `GET /v1/admin/maintenance:usage-drift` — reports only.
pub(crate) async fn usage_drift_route(State(state): State<Arc<FastcoreState>>) -> Response {
    run(state, true).await
}

/// `POST /v1/admin/maintenance:usage-drift` — reports, and with
/// `?dry_run=false` sets each counter to its disk figure and deletes the
/// counters of addresses that are not accounts.
pub(crate) async fn usage_drift_apply_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<UsageDriftQuery>,
) -> Response {
    run(state, q.dry_run).await
}
