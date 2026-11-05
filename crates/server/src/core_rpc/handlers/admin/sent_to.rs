//! The list of addresses each user sent mail to, which the fraud rules
//! read as "someone the user wrote to", rebuilt from the relationship
//! counters in the shared store. The same pass as fastcore, through
//! `core_sidestate::families::contact_sent`.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use mailrs_core_api::method::admin as wire;
use mailrs_core_sidestate::families::contact_sent;

use super::quota::with_net;
use crate::core_rpc::CoreRpcState;

#[derive(serde::Deserialize)]
pub struct SentToQuery {
    #[serde(default = "dry_run_unless_asked")]
    dry_run: bool,
}

fn dry_run_unless_asked() -> bool {
    true
}

/// POST /v1/admin/maintenance:backfill-sent-to — `?dry_run=false` writes.
pub async fn backfill_sent_to_route(
    State(state): State<Arc<CoreRpcState>>,
    Query(q): Query<SentToQuery>,
) -> Result<Json<wire::SentToBackfillResponse>, StatusCode> {
    let dry = q.dry_run;
    let done = tokio::task::spawn_blocking(move || {
        with_net(&state, |c| contact_sent::rebuild_sent_to(c, dry))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    done?
        .map(Json)
        // no shared store: there are no counters to read
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)
}
