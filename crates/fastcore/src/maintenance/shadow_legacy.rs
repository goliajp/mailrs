//! The legacy-zset shadow: what the hand-maintained indexes hold against
//! what the declared table says. Its verdict is what let the zsets go.

use super::prelude::*;

mod axes;
mod bucket_axis;

/// `POST /v1/admin/maintenance:shadow-read` — compare the ORDERPATH's
/// answer with the zset's for every account, without serving either.
///
/// The zsets stay authoritative through this phase. This is the only
/// step that can show the two agree on **content and order** before a
/// read is cut over; `TABLE.VERIFY` proves the index matches the rows,
/// which is a different claim — the rows themselves could be wrong.
///
/// Divergence is reported per user rather than summed, because one
/// account disagreeing is a different problem from all of them.
pub(crate) async fn shadow_read_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let limit: usize = q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(200);
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let store = state.mailbox.store_ref();
    let mut report = Vec::new();
    let mut total_divergent = 0u64;

    for user in &users {
        let found = axes::divergences_for_user(&state, store, user, limit);
        total_divergent += found.len() as u64;
        report.extend(found);
    }

    Json(serde_json::json!({
        "limit": limit,
        "axes_checked": users.len() * 16,
        "axes_divergent": total_divergent,
        "divergences": report,
    }))
    .into_response()
}
