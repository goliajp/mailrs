//! `/metrics` — Prometheus exposition of the process's `metrics`
//! recorder. webapi installs its own recorder at boot via
//! `metrics-exporter-prometheus`.
//!
//! The route sits behind the public reverse proxy, so it answers only a
//! scraper that sends `Authorization: Bearer <MAILRS_METRICS_TOKEN>`.
//! With no token configured the route is off and answers 404, like a
//! path that does not exist.

use std::sync::{Arc, OnceLock};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use subtle::ConstantTimeEq;

use crate::WebState;

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

/// GET /metrics — Prometheus text/plain output. 404 when no token is
/// configured, 401 when the bearer token is missing or wrong. Returns
/// 503 if [`install`] wasn't called (the standard boot path always
/// calls it).
pub async fn prometheus_metrics(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let Some(expected) = state.metrics_token.as_deref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !bearer_matches(&headers, expected) {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer")],
        )
            .into_response();
    }
    match HANDLE.get() {
        Some(h) => {
            let mut body = h.render();
            body.push_str(&mock_metrics());
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
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

// a length mismatch returns early: the token's length is not secret, its bytes are
fn bearer_matches(headers: &HeaderMap, expected: &str) -> bool {
    let Some(given) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.as_bytes().strip_prefix(b"Bearer "))
    else {
        return false;
    };
    given.ct_eq(expected.as_bytes()).into()
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
