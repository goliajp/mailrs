//! `/metrics` is reachable through the public reverse proxy, so it answers
//! only a scraper holding `MAILRS_METRICS_TOKEN`, and not at all when no
//! token is configured.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

fn router(metrics_token: Option<&str>) -> axum::Router {
    mailrs_webapi::handlers::metrics::install();
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
        metrics_token: metrics_token.map(str::to_string),
    });
    mailrs_webapi::build_router(state)
}

async fn scrape(metrics_token: Option<&str>, authorization: Option<&str>) -> (StatusCode, String) {
    let mut req = Request::get("/metrics");
    if let Some(a) = authorization {
        req = req.header("authorization", a);
    }
    let resp = router(metrics_token)
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn without_a_configured_token_the_route_is_not_found() {
    let (status, body) = scrape(None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(!body.contains("# TYPE"), "{body}");

    let (status, _) = scrape(None, Some("Bearer anything")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_missing_token_is_refused() {
    let (status, body) = scrape(Some("s3cret-token"), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(!body.contains("# TYPE"), "{body}");
}

#[tokio::test]
async fn a_wrong_token_is_refused() {
    for given in [
        "Bearer s3cret-tokeX",
        "Bearer s3cret-token-longer",
        "Bearer s3cret",
        "Bearer ",
        "Basic s3cret-token",
        "s3cret-token",
    ] {
        let (status, _) = scrape(Some("s3cret-token"), Some(given)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{given}");
    }
}

#[tokio::test]
async fn the_right_token_gets_the_metrics() {
    // a counter emitted before the recorder exists goes nowhere
    mailrs_webapi::handlers::metrics::install();
    metrics::counter!("metrics_token_test_total").increment(1);
    let (status, body) = scrape(Some("s3cret-token"), Some("Bearer s3cret-token")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("metrics_token_test_total"), "{body}");
}
