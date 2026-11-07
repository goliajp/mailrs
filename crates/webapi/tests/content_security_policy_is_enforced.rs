//! With `MAILRS_CSP_MODE=enforce` the same policy goes out in the header
//! browsers enforce, and not in the report-only one. Its own test binary:
//! the mode is fixed once per process.

use std::sync::Arc;

use axum::body::Body;
use axum::http::Request;
use mailrs_webapi::handlers::csp_report::{self, Mode};
use tower::ServiceExt;

#[tokio::test]
async fn the_policy_is_enforced_when_asked_for() {
    csp_report::set_mode(Mode::Enforce);
    let state = Arc::new(mailrs_webapi::WebState {
        core: Arc::new(mailrs_core_api::client::Client::new(
            "http://127.0.0.1:1",
            "test-secret",
        )),
        bind_addr: "127.0.0.1:0".into(),
        event_bus: std::sync::OnceLock::new(),
        trace_bus: std::sync::OnceLock::new(),
        receiver_stats: Arc::new(mailrs_webapi::handlers::smtp_monitor::StatsCache::default()),
        started_at: std::time::Instant::now(),
        llm_config: None,
        metrics_token: None,
    });
    let res = mailrs_webapi::build_router(state)
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let policy = res
        .headers()
        .get("content-security-policy")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(policy, csp_report::POLICY);
    assert!(
        res.headers()
            .get("content-security-policy-report-only")
            .is_none()
    );
}
