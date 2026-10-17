//! An unknown `/api` path answers 404 with a JSON error. It used to reach
//! the web app's fallback and come back as index.html with a 200, so a
//! client calling a wrong path parsed HTML and reported a schema error.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

fn router() -> axum::Router {
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
    });
    mailrs_webapi::build_router(state)
}

async fn get(path: &str) -> (StatusCode, String) {
    let res = router()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn an_unknown_api_path_is_a_json_404() {
    let (status, body) = get("/api/api/admin/dmarc/reports").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        body.contains("no API route /api/api/admin/dmarc/reports"),
        "{body}"
    );
}

#[tokio::test]
async fn known_routes_still_answer() {
    let (status, _) = get("/api/health").await;
    assert_ne!(status, StatusCode::NOT_FOUND);
    // an authenticated route is still reached (and refuses the missing
    // session) rather than being swallowed by the fallback
    let (status, _) = get("/api/mock/namespaces/x/messages").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
