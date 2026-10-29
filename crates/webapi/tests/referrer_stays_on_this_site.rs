//! Every response tells the browser to send no `Referer` to another site:
//! the address of a mail page names a thread, and the links inside a
//! message lead anywhere. `same-origin`, not `no-referrer`, which makes a
//! browser send `Origin: null` on our own POSTs and fails the origin checks.

use std::sync::Arc;

use axum::body::Body;
use axum::http::Request;
use tower::ServiceExt;

#[tokio::test]
async fn every_response_keeps_the_referrer_on_this_site() {
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
    let router = mailrs_webapi::build_router(state);
    for path in ["/api/no-such-route", "/api/auth/me", "/api/mail/inline/x"] {
        let res = router
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            res.headers()
                .get("referrer-policy")
                .and_then(|v| v.to_str().ok()),
            Some("same-origin"),
            "{path} answered {} without it",
            res.status()
        );
    }
}
