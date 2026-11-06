//! `GET /api/admin/push-stats?days=14`: pushes sent, failed and held back,
//! per channel per day, summed over every account that exists now. Counts
//! only, no per-person rows, so any administrator may read it.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use mailrs_core_sidestate::families::notify;
use serde::{Deserialize, Serialize};

use crate::WebState;
use crate::handlers::admin::map_err;
use crate::handlers::kevy_util::with_kevy;

#[derive(Debug, Deserialize)]
pub struct PushStatsQuery {
    #[serde(default = "fourteen")]
    pub days: u32,
}

fn fourteen() -> u32 {
    14
}

#[derive(Debug, Serialize)]
pub struct PushStats {
    /// how many accounts the sums are over
    pub accounts: usize,
    /// newest day first
    pub days: Vec<notify::SiteDay>,
}

/// GET /api/admin/push-stats — at most 90 days, as long as counters are kept.
pub async fn push_stats(
    State(state): State<Arc<WebState>>,
    Query(q): Query<PushStatsQuery>,
) -> Result<Json<PushStats>, StatusCode> {
    let days = q.days.clamp(1, 90);
    let users: Vec<String> = state
        .core
        .list_accounts()
        .await
        .map_err(map_err)?
        .items
        .into_iter()
        .map(|a| a.address)
        .collect();
    let accounts = users.len();
    let now = chrono::Utc::now().timestamp();
    let days = with_kevy(move |c| notify::site_stats(c, &users, days, now))?;
    Ok(Json(PushStats { accounts, days }))
}
