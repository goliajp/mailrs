//! Prometheus metrics recorder + handle exposed via `/metrics`.
//!
//! Uses the `metrics` facade so call sites can stay agnostic of the
//! exporter (`metrics::counter!()` / `gauge!()` / `histogram!()`). The
//! exporter here is `metrics-exporter-prometheus`, configured in
//! "BuildRecorder + manual handle" mode: we keep the
//! `PrometheusHandle` around so the axum `/metrics` route can render
//! the snapshot on demand without spawning a background HTTP server
//! inside the exporter itself.
//!
//! Why not the exporter's built-in HTTP server: we already have axum,
//! the mailrs hostname + TLS + auth middleware live there, and a
//! second hyper listener for `/metrics` would force us to duplicate
//! all of that. The render-on-demand path costs ~tens of µs per
//! scrape — fine for Prometheus pull intervals of 5-60s.

use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

/// Install the prometheus recorder as the global `metrics::Recorder`
/// and return a handle the axum `/metrics` route can use to render
/// the current snapshot.
///
/// **Idempotency:** `metrics` allows the global recorder to be
/// installed exactly once per process. Calling this twice in the
/// same process panics inside the `metrics` crate. The server's
/// `bootstrap` flow guarantees one call from `main()`; tests should
/// not invoke this function directly.
pub fn install_prometheus_recorder() -> PrometheusHandle {
    let builder = PrometheusBuilder::new();
    // `install_recorder` returns the handle and installs the
    // recorder atomically — no race between "recorder live" and
    // "handle available".
    builder
        .install_recorder()
        .expect("failed to install prometheus recorder")
}
