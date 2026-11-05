//! Purging deleted email groups on the SQL core: the network kevy's part,
//! shared with fastcore (`group_purge`), and the reply claims of the
//! groups' conversations in this core's table, as fastcore removes them
//! from its store.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use mailrs_core_api::method::admin::{GroupPurgeResponse, PurgeCount};
use mailrs_core_sidestate::families::group_purge;

use super::account_purge::{net, now, sql_failed, yes};
use crate::core_rpc::CoreRpcState;

#[derive(Debug, serde::Deserialize)]
pub struct GroupPurgeQuery {
    #[serde(default = "yes")]
    dry_run: bool,
    gid: Option<i64>,
}

/// The table a group purge counts this core's reply claims under.
const REPLY_TABLE: &str = "shared_reply_state";

/// One group pass: the network kevy's part, and the reply claims of the
/// groups' conversations in this core's table, as fastcore's.
pub(crate) async fn run_groups(
    state: &Arc<CoreRpcState>,
    dry_run: bool,
    only: Option<i64>,
    now: i64,
    retention_days: u64,
) -> Result<GroupPurgeResponse, StatusCode> {
    let pass = net(state, move |c| {
        group_purge::due(c, now, retention_days, only)
    })
    .await?;
    let mut rows = Vec::new();
    for g in &pass.due {
        let n = state
            .mailbox
            .purge_group_replies(g.gid, dry_run)
            .await
            .map_err(sql_failed)?;
        let mail = vec![PurgeCount {
            name: REPLY_TABLE.to_string(),
            count: n,
        }];
        let g = g.clone();
        let row = net(state, move |c| {
            let row = group_purge::row(&g, group_purge::network(c, &g, dry_run)?, mail);
            if !dry_run {
                group_purge::finish(c, &row, now)?;
            }
            Ok(row)
        })
        .await?;
        rows.push(row);
    }
    Ok(pass.response(dry_run, retention_days, rows))
}

/// POST /v1/admin/maintenance:group-purge
pub async fn group_purge_route(
    State(state): State<Arc<CoreRpcState>>,
    Query(q): Query<GroupPurgeQuery>,
) -> Result<Json<GroupPurgeResponse>, StatusCode> {
    let days = group_purge::retention_days();
    run_groups(&state, q.dry_run, q.gid, now(), days)
        .await
        .map(Json)
}
