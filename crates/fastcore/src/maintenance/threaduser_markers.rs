//! The membership rows' index markers: fill them in on rows written before
//! they existed, and check that every one agrees with its column.
//!
//! The reads key on the marker indexes alone, so a row whose marker
//! disagrees with its column is listed wrongly; the check must read zero.

use super::prelude::*;

#[derive(serde::Deserialize)]
pub(crate) struct RewriteQuery {
    #[serde(default)]
    offset: usize,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    2000
}

/// `POST /v1/admin/maintenance:rewrite-threaduser-markers?offset=&limit=`
///
/// One page of membership rows, in key order. Writes only where a marker
/// disagrees with its column, so a second pass reports `changed: 0`.
pub(crate) async fn rewrite_threaduser_markers_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<RewriteQuery>,
) -> axum::response::Response {
    let (offset, limit) = (q.offset, q.limit);
    let work =
        move |_: &super::hangup::Hangup| state.mailbox.rewrite_threaduser_markers(offset, limit);
    match super::hangup::run_blocking(work).await {
        Ok(Ok(r)) => {
            tracing::info!(
                offset,
                walked = r.walked,
                changed = r.changed,
                "rewrite-threaduser-markers"
            );
            Json(serde_json::json!({
                "total_rows": r.total_rows,
                "offset": offset,
                "limit": limit,
                "walked": r.walked,
                "changed": r.changed,
                "already_correct": r.already_correct,
                "next_offset": offset + r.walked,
                "done": offset + r.walked >= r.total_rows,
            }))
            .into_response()
        }
        Ok(Err(e)) => {
            tracing::error!(error = %e, "rewrite-threaduser-markers failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "rewrite-threaduser-markers task failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `POST /v1/admin/maintenance:threaduser-marker-check` — read-only.
///
/// For each marked column, the rows with the column non-zero against the
/// rows carrying its marker, plus the marker index's own entry count.
/// `mismatches` is zero everywhere when the two sets are the same.
pub(crate) async fn threaduser_marker_check_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    let work = move |_: &super::hangup::Hangup| state.mailbox.threaduser_marker_check();
    match super::hangup::run_blocking(work).await {
        Ok(Ok((rows, columns))) => {
            let mismatches: usize = columns.iter().map(|c| c.mismatches).sum();
            Json(serde_json::json!({
                "rows_walked": rows,
                "mismatches": mismatches,
                "columns": columns,
            }))
            .into_response()
        }
        Ok(Err(e)) => {
            tracing::error!(error = %e, "threaduser-marker-check failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "threaduser-marker-check task failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
