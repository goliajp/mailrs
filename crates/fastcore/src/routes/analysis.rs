//! Email-analysis routes served from the network kevy so both cores read
//! the identical analysis rows:
//!   `mailrs:analysis:{message_id}`      JSON EmailAnalysisWire
//!   `mailrs:analysis:unanalyzed`        set of message_ids awaiting analysis
//!   `mailrs:attachments:{message_id}`   list of extracted attachment texts
//!
//! `message_id` is the i64 the wire uses; the analyzer worker writes these
//! keys, both cores read them. `semantic_search` returns 501 on both cores
//! (the Phase-2 wire surface has no vector backend).

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;

use mailrs_core_api::method::analysis::{
    AttachmentTextsResponse, CountUnanalyzedResponse, EmailAnalysisWire, GetAnalysisResponse,
    ListUnanalyzedQuery,
};

use crate::FastcoreState;

pub async fn get_analysis(
    State(state): State<Arc<FastcoreState>>,
    Path(message_id): Path<i64>,
) -> Json<GetAnalysisResponse> {
    let analysis = state.net_conn().and_then(|mut conn| {
        conn.get(format!("mailrs:analysis:{message_id}").as_bytes())
            .ok()
            .flatten()
            .and_then(|v| serde_json::from_slice::<EmailAnalysisWire>(&v).ok())
    });
    Json(GetAnalysisResponse { analysis })
}

pub async fn count_unanalyzed(
    State(state): State<Arc<FastcoreState>>,
    Query(_q): Query<ListUnanalyzedQuery>,
) -> Json<CountUnanalyzedResponse> {
    let count = state
        .net_conn()
        .and_then(|mut conn| conn.scard(b"mailrs:analysis:unanalyzed").ok())
        .unwrap_or(0) as i64;
    Json(CountUnanalyzedResponse { count })
}

pub async fn boost_importance(
    State(state): State<Arc<FastcoreState>>,
    Path(message_id): Path<i64>,
) -> StatusCode {
    let Some(mut conn) = state.net_conn() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let key = format!("mailrs:analysis:{message_id}");
    // read-modify-write the stored analysis row's importance signal
    if let Ok(Some(v)) = conn.get(key.as_bytes())
        && let Ok(mut row) = serde_json::from_slice::<EmailAnalysisWire>(&v)
    {
        row.requires_action = true;
        if let Ok(json) = serde_json::to_vec(&row) {
            let _ = conn.set(key.as_bytes(), &json);
        }
    }
    StatusCode::NO_CONTENT
}

pub async fn attachment_texts(
    State(state): State<Arc<FastcoreState>>,
    Path(message_id): Path<i64>,
) -> Json<AttachmentTextsResponse> {
    let texts = state
        .net_conn()
        .and_then(|mut conn| {
            conn.lrange(format!("mailrs:attachments:{message_id}").as_bytes(), 0, -1)
                .ok()
        })
        .unwrap_or_default()
        .into_iter()
        .map(|v| String::from_utf8_lossy(&v).into_owned())
        .collect();
    Json(AttachmentTextsResponse { texts })
}

/// Semantic search — 501 on both cores (no vector backend on this wire).
pub async fn semantic_search() -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}
