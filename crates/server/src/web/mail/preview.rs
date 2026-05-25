//! Email render preview (headless Chrome) + render cache serving.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;

use super::{AuthUser, WebState};

#[derive(Deserialize)]
pub(crate) struct RenderPreviewRequest {
    pub html: String,
    #[serde(default)]
    pub presets: Vec<String>,
}

pub(crate) async fn render_preview(
    _auth: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<RenderPreviewRequest>,
) -> impl IntoResponse {
    let Some(ref client) = state.render_preview else {
        return Json(serde_json::json!({
            "error": "rendering engine not configured (MAILRS_CHROME_CDP_URL not set)"
        }))
        .into_response();
    };

    if req.html.len() > 1_000_000 {
        return Json(serde_json::json!({"error": "html too large (max 1MB)"})).into_response();
    }

    tracing::debug!(
        event = "render_preview_start",
        html_len = req.html.len(),
        presets = ?req.presets
    );
    let results = client.render(&req.html, &req.presets).await;

    let mut previews = Vec::new();
    let mut errors = Vec::new();
    for result in results {
        match result {
            Ok(preview) => previews.push(preview),
            Err(e) => {
                tracing::warn!(event = "render_preview_partial_error", error = %e);
                errors.push(e);
            }
        }
    }
    tracing::debug!(
        event = "render_preview_done",
        ok_count = previews.len(),
        error_count = errors.len()
    );

    Json(serde_json::json!({
        "previews": previews,
        "errors": errors,
    }))
    .into_response()
}

pub(crate) async fn serve_render_cache(
    Path(id): Path<String>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    use axum::http::header;

    let Some(ref client) = state.render_preview else {
        return StatusCode::NOT_FOUND.into_response();
    };

    match client.get_cached(&id).await {
        Some(data) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "image/png".to_string()),
                (header::CACHE_CONTROL, "public, max-age=3600".to_string()),
            ],
            data,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
