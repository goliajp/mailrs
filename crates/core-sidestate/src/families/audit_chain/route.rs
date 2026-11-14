//! `POST /v1/admin/maintenance:audit-verify`: the audit chain checked now,
//! and compared with its replica when one is configured. Read-only: it
//! records nothing and moves nothing, unlike the daily check. The chain
//! lives in the network kevy, so every core serves this alike.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::NetKevy;
use crate::families::audit;

pub async fn verify_route<S: NetKevy>(State(state): State<Arc<S>>) -> Response {
    let Some(mut c) = state.net_conn() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "no network kevy: the audit log lives there",
        )
            .into_response();
    };
    let checked = tokio::task::spawn_blocking(move || {
        let chain = super::verify(&mut c, audit::CAP)?;
        let replica = match std::env::var("MAILRS_AUDIT_REPLICA_DIR")
            .ok()
            .filter(|d| !d.is_empty())
        {
            None => serde_json::json!("off"),
            Some(dir) => match super::compare_replica(std::path::Path::new(&dir), &mut c)? {
                Ok(compared) => serde_json::json!({ "ok": true, "compared": compared }),
                Err((id, why)) => {
                    serde_json::json!({ "ok": false, "first_bad_id": id, "reason": why })
                }
            },
        };
        std::io::Result::Ok(serde_json::json!({ "chain": chain, "replica": replica }))
    })
    .await;
    match checked {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => {
            tracing::error!(error = %e, "audit-verify: the store could not be read");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
