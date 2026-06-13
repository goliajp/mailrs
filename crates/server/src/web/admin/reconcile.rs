//! Maildir → PG reconciliation endpoint (split-brain read-repair).
//!
//! POST /api/admin/reconcile-maildir { "dry_run": bool }
//! Permission: internal.rpc — this walks every mailbox on disk and
//! (without dry_run) writes index rows; strictly an operator tool.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::response::IntoResponse;
use serde::Deserialize;

use super::{AuthUser, WebState, require_permission};

#[derive(Deserialize)]
pub(crate) struct ReconcileRequest {
    /// report the gap without writing anything
    #[serde(default)]
    pub dry_run: bool,
}

pub(crate) async fn reconcile_maildir(
    AuthUser {
        ref address,
        ref permissions,
        ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(body): Json<ReconcileRequest>,
) -> impl IntoResponse {
    if let Some(resp) = require_permission(permissions, "internal.rpc") {
        return resp.into_response();
    }
    let Some(ref store) = state.mailbox_store else {
        return Json(serde_json::json!({"error": "mailbox store not available"})).into_response();
    };
    tracing::info!(
        event = "reconcile_maildir_started",
        actor = %address,
        dry_run = body.dry_run,
        maildir_root = %state.maildir_root,
    );
    match store
        .reconcile_maildir(&state.maildir_root, body.dry_run)
        .await
    {
        Ok(report) => {
            tracing::info!(
                event = "reconcile_maildir_finished",
                scanned = report.scanned,
                missing = report.missing,
                repaired = report.repaired,
                errors = report.errors.len(),
            );
            Json(serde_json::json!({
                "dry_run": body.dry_run,
                "report": report,
            }))
            .into_response()
        }
        Err(e) => Json(serde_json::json!({"error": e})).into_response(),
    }
}
