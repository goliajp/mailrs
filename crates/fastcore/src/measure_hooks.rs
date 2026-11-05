//! Built only with `--features measure-hooks`: starts one round of a
//! periodic task now, on the runtime the timer runs it on, so its effect
//! on serving can be measured without waiting for the timer.
use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
};

use crate::FastcoreState;

#[derive(serde::Deserialize)]
pub(crate) struct Run {
    /// `heal` (the full maildir pass), `aof` (a rewrite when due) or
    /// `trash` (a purge of up to 500 conversations trashed before now).
    task: String,
}

/// `POST /v1/admin/debug:run-periodic?task=…` — answers at once; the
/// round logs `measure hook: done` with its length.
pub(crate) async fn run_periodic(
    State(state): State<Arc<FastcoreState>>,
    Query(run): Query<Run>,
) -> StatusCode {
    let task = run.task;
    if !matches!(task.as_str(), "heal" | "aof" | "trash") {
        return StatusCode::BAD_REQUEST;
    }
    crate::maintenance::offload::spawn(async move {
        tracing::info!(%task, "measure hook: start");
        let started = std::time::Instant::now();
        let outcome = match task.as_str() {
            "heal" => format!(
                "repaired={}",
                crate::ingest_loop::heal_pass(&state, 0).await
            ),
            "aof" => {
                let dir = std::env::var("MAILRS_KEVY_DATA_DIR").unwrap_or_default();
                format!(
                    "compacted={:?}",
                    crate::aof_compact::compact_once(&state, &dir, None)
                )
            }
            _ => {
                let now = chrono::Utc::now().timestamp();
                let deleted = crate::routes::trash::purge_once(&state, now, 500).await;
                format!("deleted={deleted}")
            }
        };
        tracing::info!(%task, %outcome, elapsed_ms = started.elapsed().as_millis() as u64,
            "measure hook: done");
    });
    StatusCode::ACCEPTED
}
