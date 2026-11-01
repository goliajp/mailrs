//! The membership rows' index markers: fill them in on rows written before
//! they existed, and check that every one agrees with its column.
//!
//! The check is what has to read zero mismatches on production before the
//! full-row flag indexes are dropped (`KEEP_FULL_ROW_FLAG_INDEXES`).

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

/// `POST /v1/admin/maintenance:threaduser-read-compare` — read-only.
///
/// Every flag read, for every user, off the marker index and off the
/// full-row index, and each timestamp index's keys off both. Answers only
/// while the full-row indexes are declared. `differences` is zero when the
/// two return the same; `rows_returned` says something was compared.
pub(crate) async fn threaduser_read_compare_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    let work = move |_: &super::hangup::Hangup| state.mailbox.threaduser_read_compare();
    match super::hangup::run_blocking(work).await {
        Ok(Ok(r)) => Json(r).into_response(),
        Ok(Err(e)) => {
            tracing::error!(error = %e, "threaduser-read-compare failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "threaduser-read-compare task failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
