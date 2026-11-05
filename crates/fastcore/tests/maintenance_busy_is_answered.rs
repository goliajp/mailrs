//! With every maintenance worker holding a request, the next maintenance
//! request is answered at once with `429` and what is running, instead of
//! waiting behind them.
//!
//! The latch is the network kevy: a listener that takes each walk's
//! connection, reads its first command and does not answer until the test
//! has seen the third request answered.
use std::{
    io::Read,
    net::TcpListener,
    sync::{Arc, mpsc},
    time::Duration,
};

use http_body_util::BodyExt;
use mailrs_fastcore::FastcoreState;
use mailrs_mailbox_kevy::KevyMailboxStore;
use tower::ServiceExt;

const WALK: &str = "/v1/admin/maintenance:quota-limits";

async fn call(router: axum::Router) -> (axum::http::StatusCode, serde_json::Value) {
    let response = router
        .oneshot(
            axum::http::Request::post(WALK)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
    )
}

#[test]
fn a_maintenance_request_past_the_workers_is_told_it_is_busy() {
    let kevy = TcpListener::bind("127.0.0.1:0").unwrap();
    // One test in its own integration-test process owns this variable.
    unsafe {
        std::env::set_var(
            "MAILRS_KEVY_URL",
            format!("kevy://{}", kevy.local_addr().unwrap()),
        );
    }
    let store = KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    store.ensure_thread_table();
    store.ensure_admin_indexes();
    store
        .upsert_account(
            "bob@x.com",
            r#"{"address":"bob@x.com","active":true,"display_name":"Bob"}"#,
        )
        .unwrap();
    let router = mailrs_fastcore::build_router(Arc::new(FastcoreState::new(store)));
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();

    // two walks, each parked on its first kevy reply
    let walks: Vec<_> = (0..2).map(|_| rt.spawn(call(router.clone()))).collect();
    let mut held = Vec::new();
    for _ in 0..2 {
        let (mut conn, _) = kevy.accept().unwrap();
        let mut first = [0u8; 1];
        conn.read_exact(&mut first).unwrap();
        held.push(conn);
    }

    let (tx, rx) = mpsc::channel();
    rt.spawn({
        let router = router.clone();
        async move { tx.send(call(router).await).unwrap() }
    });
    // Bounded only so a regression fails instead of hanging: a third
    // request that waits for a worker is not answered while the two hold.
    let third = rx.recv_timeout(Duration::from_secs(60));
    let both_still_walking = walks.iter().all(|w| !w.is_finished());

    drop(held);
    drop(kevy);
    for w in walks {
        rt.block_on(w).unwrap();
    }
    let (status, body) = third.expect("the third request waited instead of being told");
    assert_eq!(status, 429, "{body}");
    assert_eq!(body["error"], "busy");
    let running = body["running"].as_array().unwrap();
    assert_eq!(running.len(), 2, "{body}");
    assert!(
        running
            .iter()
            .all(|r| r["route"] == WALK && r["started_at"].is_i64())
    );
    assert!(
        both_still_walking,
        "a walk had finished before the third request was answered"
    );

    // the places are given back: a request now is admitted and runs
    let (status, _) = rt.block_on(call(router));
    assert_ne!(status, 429);
}
