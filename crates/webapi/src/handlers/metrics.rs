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
            let mut body = h.render();
            body.push_str(&mock_metrics());
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

// read from the shared store at scrape time: the drain that counts them
// runs in fastcore, which exports nothing
fn mock_metrics() -> String {
    use mailrs_core_sidestate::families::mock;
    if mock::domain().is_empty() {
        return String::new();
    }
    let mut stats = None;
    let _ = crate::handlers::kevy_util::with_kevy(|c| {
        stats = mock::stats(c).ok();
        Ok(())
    });
    let Some(s) = stats else {
        return String::new();
    };
    [
        ("mock_received_total", "counter", s.received),
        (
            "mock_rcpt_rejected_unknown_ns_total",
            "counter",
            s.rejected_unknown_ns,
        ),
        ("mock_store_bytes", "gauge", s.bytes),
        ("mock_dropped_total", "gauge", s.dropped),
    ]
    .iter()
    .map(|(name, kind, v)| format!("# TYPE {name} {kind}\n{name} {v}\n"))
    .collect()
}
