//! Account and alias administration.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};

use crate::*;

#[derive(serde::Deserialize)]
pub(crate) struct AliasBody {
    pub(crate) source: String,
    pub(crate) target: String,
}

/// Run `f` on the shared store. Without a configured one (tests, a
/// degraded deploy) there is no quota enforcement to feed: nothing runs.
pub(crate) fn with_net<T>(
    state: &FastcoreState,
    f: impl FnOnce(&mut kevy_client::Connection) -> std::io::Result<T>,
) -> Result<Option<T>, axum::http::StatusCode> {
    if state.net_url.is_none() {
        return Ok(None);
    }
    let mut c = state
        .net_conn()
        .ok_or(axum::http::StatusCode::SERVICE_UNAVAILABLE)?;
    f(&mut c).map(Some).map_err(|e| {
        tracing::warn!(error = %e, "shared store write failed");
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    })
}

#[derive(serde::Deserialize)]
pub(crate) struct QuotaLimitsQuery {
    #[serde(default = "dry_run_unless_asked")]
    dry_run: bool,
}

fn dry_run_unless_asked() -> bool {
    true
}

/// `POST /v1/admin/maintenance:quota-limits` — every account's usage
/// against the default limit; `?dry_run=false` gives the default to the
/// accounts with no limit that are not near full.
pub(crate) async fn quota_limits_route(
    State(state): State<Arc<FastcoreState>>,
    axum::extract::Query(q): axum::extract::Query<QuotaLimitsQuery>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::warn!(error = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let pass = with_net(&state, |c| {
        mailrs_core_sidestate::families::quota_limits::fill_in(c, &users, q.dry_run)
    });
    match pass {
        Ok(Some(report)) => Json(report).into_response(),
        // no shared store: nowhere a limit could be read from or written to
        Ok(None) => axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response(),
        Err(code) => code.into_response(),
    }
}

/// GET `/v1/admin/aliases:local` — list every fastcore-embedded alias.
pub(crate) async fn list_local_aliases(
    State(state): State<Arc<FastcoreState>>,
) -> Json<serde_json::Value> {
    let items = state.alias_store.list().unwrap_or_default();
    let payload: Vec<serde_json::Value> = items
        .into_iter()
        .map(|(source, target)| serde_json::json!({"source": source, "target": target}))
        .collect();
    Json(serde_json::json!({ "items": payload }))
}

/// POST `/v1/admin/aliases:local` — insert/replace one alias entry.
pub(crate) async fn upsert_local_alias(
    State(state): State<Arc<FastcoreState>>,
    Json(body): Json<AliasBody>,
) -> axum::response::Response {
    if body.source.is_empty() || body.target.is_empty() {
        return axum::http::StatusCode::BAD_REQUEST.into_response();
    }
    match state.alias_store.upsert(&body.source, &body.target) {
        Ok(()) => axum::http::StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::error!(err = %e, source = %body.source, "upsert_alias failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// DELETE `/v1/admin/aliases:local/{source}` — drop one alias entry.
pub(crate) async fn delete_local_alias_route(
    State(state): State<Arc<FastcoreState>>,
    Path(source): Path<String>,
) -> axum::response::Response {
    match state.alias_store.delete(&source) {
        Ok(_) => axum::http::StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::error!(err = %e, %source, "delete_alias failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
