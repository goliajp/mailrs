//! `/metrics` — Prometheus exposition of the process's `metrics`
//! recorder. Mirrors the monolith at `admin::prometheus_metrics`;
//! webapi installs its own recorder at boot via
//! `metrics-exporter-prometheus`.

use std::sync::OnceLock;

use axum::http::StatusCode;
use axum::response::IntoResponse;

use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Install the Prometheus recorder once. Idempotent: subsequent calls
/// are no-ops. Call from webapi::run() at boot.
pub fn install() {
    HANDLE.get_or_init(|| {
        PrometheusBuilder::new()
            .install_recorder()
            .expect("install Prometheus recorder")
    });
}

/// GET /metrics — Prometheus text/plain output. Returns 503 if
/// [`install`] wasn't called (defensive; the standard boot path always
/// calls it).
pub async fn prometheus_metrics() -> impl IntoResponse {
    match HANDLE.get() {
        Some(h) => {
            let body = h.render();
            (
                StatusCode::OK,
                [(
                    axum::http::header::CONTENT_TYPE,
                    "text/plain; version=0.0.4",
                )],
                body,
            )
                .into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            "metrics recorder not initialised",
        )
            .into_response(),
    }
}
