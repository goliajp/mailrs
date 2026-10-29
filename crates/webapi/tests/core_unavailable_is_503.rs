//! While the core is restarting — refusing connections as it replays its
//! log during a deploy — webapi answers 503 with `Retry-After`, not 500:
//! nothing is wrong with the request, and a read repeated shortly will work.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, header};
use tower::ServiceExt;

#[tokio::test]
async fn an_unreachable_core_is_a_503_with_retry_after() {
    let state = Arc::new(mailrs_webapi::WebState {
        // nothing listens here: every call to the core is refused
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
    let res = mailrs_webapi::build_router(state)
        .oneshot(
            Request::post("/api/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"address":"a@example.test","password":"pw"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), 503);
    assert_eq!(
        res.headers()
            .get(header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok()),
        Some("5")
    );
}
