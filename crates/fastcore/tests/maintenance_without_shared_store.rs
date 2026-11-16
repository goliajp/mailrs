//! The maintenance passes over the shared store answer 503 when it is not
//! configured or cannot be reached: there is nothing they could read or
//! write, and a caller retrying later is the right response. Both cores
//! serve these routes from one shared handler, so this holds for both.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

const ROUTES: [(Method, &str); 7] = [
    (Method::POST, "/v1/admin/maintenance:quota-limits"),
    (Method::GET, "/v1/admin/maintenance:usage-drift"),
    (Method::POST, "/v1/admin/maintenance:usage-drift"),
    (Method::POST, "/v1/admin/maintenance:backfill-sent-to"),
    (Method::GET, "/v1/admin/maintenance:sieve-survey"),
    (Method::POST, "/v1/admin/maintenance:account-purge"),
    (Method::POST, "/v1/admin/maintenance:group-purge"),
];

async fn answers(net_url: Option<&str>) -> Vec<(String, StatusCode)> {
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"),
    ));
    mailbox.ensure_admin_indexes();
    let mut state = mailrs_fastcore::FastcoreState::new(mailbox);
    state.net_url = net_url.map(str::to_string);
    let app = mailrs_fastcore::build_router(Arc::new(state));
    let mut out = Vec::new();
    for (method, path) in ROUTES {
        let req = Request::builder()
            .method(method.clone())
            .uri(path)
            .body(Body::empty())
            .expect("request");
        let res = app.clone().oneshot(req).await.expect("answer");
        out.push((format!("{method} {path}"), res.status()));
    }
    out
}

#[tokio::test]
async fn an_unreachable_shared_store_is_503_on_every_pass() {
    // nothing listens on the discard port
    for (route, status) in answers(Some("kevy://127.0.0.1:9")).await {
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{route}");
    }
}

#[tokio::test]
async fn no_shared_store_is_503_on_every_pass() {
    for (route, status) in answers(None).await {
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{route}");
    }
}
