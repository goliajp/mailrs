//! Reporting a scam: one person's report holds their copy at once, and the
//! other copies of the same template are taken back into Review, or wait
//! for an administrator (`mailrs_report::decide` says when). The routes are
//! `mailrs_core_routes::intake`; this core's half is [`fingerprint`].

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::FastcoreState;

pub(crate) mod fingerprint;

#[derive(serde::Deserialize)]
pub(crate) struct FootprintQuery {
    limit: Option<usize>,
}

/// `GET /v1/admin/maintenance:template-footprint?limit=` — read-only: how
/// many fingerprints and findable index rows are stored, the bytes their
/// fields hold, and that per ten thousand messages. For sizing on a copy:
/// it writes nothing. Bytes are the stored keys and fields, not the
/// store's own overhead; compare the store's memory before and after for
/// that.
pub(crate) async fn footprint(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<FootprintQuery>,
) -> Response {
    let limit = q.limit.unwrap_or(100_000).clamp(1, 1_000_000);
    match tokio::task::spawn_blocking(move || state.mailbox.template_footprint(limit)).await {
        Ok(Ok(f)) => {
            let per = |bytes: u64| match f.fingerprints {
                0 => serde_json::Value::Null,
                n => serde_json::json!(bytes * 10_000 / n),
            };
            Json(serde_json::json!({
                "fingerprints": f.fingerprints,
                "fingerprint_bytes": f.fingerprint_bytes,
                "segment_rows": f.segment_rows,
                "segment_bytes": f.segment_bytes,
                "index_entries": f.segment_rows * 5,
                "capped": f.capped,
                "fingerprint_bytes_per_10k": per(f.fingerprint_bytes),
                "segment_bytes_per_10k": per(f.segment_bytes),
            }))
            .into_response()
        }
        Ok(Err(e)) => {
            tracing::error!(error = %e, "template footprint failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
