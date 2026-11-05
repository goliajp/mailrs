//! A core-api call is answered while a maintenance route is in the middle of
//! its synchronous walk, on a serving runtime with a single worker.
//!
//! The latch is the network kevy: a listener that takes the route's
//! connection, reads its first command and does not answer until the test
//! has its core-api reply.
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

fn call(
    router: axum::Router,
    request: axum::http::Request<axum::body::Body>,
) -> impl Future<Output = (axum::http::StatusCode, axum::body::Bytes)> {
    async move {
        let response = router.oneshot(request).await.unwrap();
        let status = response.status();
        (
            status,
            response.into_body().collect().await.unwrap().to_bytes(),
        )
    }
}

#[test]
fn a_core_api_call_is_answered_while_a_maintenance_walk_runs() {
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
    let state = Arc::new(FastcoreState::new(store));
    let router = mailrs_fastcore::build_router(state);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let walk = rt.spawn(call(
        router.clone(),
        axum::http::Request::post("/v1/admin/maintenance:quota-limits")
            .body(axum::body::Body::empty())
            .unwrap(),
    ));

    // the walk is waiting on its first kevy reply
    let (mut conn, _) = kevy.accept().unwrap();
    let mut first = [0u8; 1];
    conn.read_exact(&mut first).unwrap();

    let (tx, rx) = mpsc::channel();
    rt.spawn(async move {
        let answer = call(
            router,
            axum::http::Request::get("/v1/users/bob%40x.com/conversations/unseen-count")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await;
        tx.send(answer).unwrap();
    });
    // Bounded only so a regression fails instead of hanging: with the walk
    // on the serving runtime's one worker this call cannot run at all.
    let answered = rx.recv_timeout(Duration::from_secs(60));
    let still_walking = !walk.is_finished();

    drop(conn);
    drop(kevy);
    rt.block_on(walk).unwrap();

    let (status, body) = answered.expect("the core-api call waited on the maintenance walk");
    assert_eq!(status, 200);
    let count: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(count["count"], 0);
    assert!(
        still_walking,
        "the walk had finished before the call was answered, so this proved nothing"
    );
}
