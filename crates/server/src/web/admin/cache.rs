//! Admin cache flush — POST /api/admin/cache/flush-conversations
//!
//! Drops every cached conversation read (thread / list / cats / action
//! prefixes) across every user. Used after a backend wire-schema change
//! so stale pre-deploy JSON stops being served; the alternative is each
//! user-per-thread tripping a separate stale-cache race.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::response::IntoResponse;

use super::*;
use crate::conversation_cache;

pub(crate) async fn flush_conversations(
    AuthUser {
        ref address,
        ref permissions,
        ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if let Some(err) = require_permission(permissions, "admin.cache") {
        return err.into_response();
    }
    let Some(ref kevy) = state.kevy_embed else {
        return Json(serde_json::json!({
            "success": false,
            "message": "kevy store not configured",
        }))
        .into_response();
    };
    let deleted = conversation_cache::bust_all_conversations(kevy);
    tracing::info!(
        target: "admin.cache",
        deleted,
        "flushed conversation caches"
    );
    if let Some(ref ds) = state.domain_store {
        ds.log_audit(
            address,
            "cache_flushed_conversations",
            "",
            &format!("deleted_keys={deleted}"),
        )
        .await;
    }
    Json(serde_json::json!({
        "success": true,
        "deleted_keys": deleted,
    }))
    .into_response()
}
