//! Every response carries the content security policy, report-only, and
//! `nosniff`. Browsers post what the policy would have blocked to
//! `/api/csp-report`, signed in or not and with no CSRF token: the browser
//! sends the report on its own and has none to add.

use std::sync::Arc;

use axum::body::Body;
use axum::http::Request;
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

#[tokio::test]
async fn the_policy_is_reported_and_its_reports_are_taken() {
    let router = router();
    for path in ["/api/health", "/api/no-such-route", "/api/auth/me"] {
        let res = router
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let header = |name: &str| {
            res.headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string()
        };
        let policy = header("content-security-policy-report-only");
        assert!(policy.contains("script-src 'self'"), "{path}: {policy:?}");
        assert!(
            policy.contains("frame-ancestors 'none'"),
            "{path}: {policy:?}"
        );
        assert!(
            policy.contains("report-uri /api/csp-report"),
            "{path}: {policy:?}"
        );
        assert_eq!(header("x-content-type-options"), "nosniff", "{path}");
    }

    let report = serde_json::json!({ "csp-report": {
        "effective-directive": "script-src-elem",
        "blocked-uri": "https://cdn.example.com/x.js?k=v",
        "document-uri": "https://mail.golia.ai/mail/thread/42",
    }});
    for cookie in [None, Some("mailrs_session=some-session-token")] {
        let mut req = Request::post("/api/csp-report")
            .header("content-type", "application/csp-report")
            .header("origin", "https://mail.golia.ai");
        if let Some(c) = cookie {
            req = req.header("cookie", c);
        }
        let res = router
            .clone()
            .oneshot(req.body(Body::from(report.to_string())).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), 204, "cookie {cookie:?}");
    }
}
