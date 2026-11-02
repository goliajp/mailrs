//! The index-marker rewrite and its check, through the routes an operator
//! runs: rows written before the markers existed are found by the check,
//! filled in page by page, a second pass changes nothing, and the check
//! reads zero after.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

const USER: &str = "bob@x.com";

async fn post(state: &Arc<mailrs_fastcore::FastcoreState>, uri: &str) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(uri)
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("call");
    assert_eq!(res.status(), StatusCode::OK, "{uri}");
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json")
}

#[tokio::test]
async fn markers_are_filled_in_once_and_checked() {
    let store =
        Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"));
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(store.clone());
    mailbox.ensure_thread_table();
    mailbox.ensure_admin_indexes();
    for i in 0..5 {
        let tid = format!("t{i}");
        mailbox
            .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
                category: "inbox",
                is_own: false,
                latest_date: 1_786_000_000 + i,
                latest_preview: "body",
                senders_csv: "a@x.com",
                subject: "s",
                thread_id: &tid,
                unread: true,
                user: USER,
            })
            .expect("arrival");
    }
    mailbox.set_starred(USER, "t1", true).expect("star");
    mailbox
        .set_trashed(USER, "t2", true, 1_786_000_100)
        .expect("trash");
    // the rows as they were before any writer kept a marker
    let markers: Vec<String> = [
        "starred",
        "archived",
        "trashed",
        "pinned",
        "unread",
        "has_action",
        "is_sender",
        "snoozed_until",
        "trashed_at",
        "awaiting_since",
    ]
    .iter()
    .map(|c| format!("ix_{c}"))
    .collect();
    let fields: Vec<&[u8]> = markers.iter().map(|m| m.as_bytes()).collect();
    for i in 0..5 {
        let key = mailrs_mailbox_kevy::keys::thread_user(USER, &format!("t{i}"));
        store.hdel(key.as_bytes(), &fields).expect("hdel");
    }
    let state = Arc::new(mailrs_fastcore::FastcoreState::new(mailbox));

    let before = post(&state, "/v1/admin/maintenance:threaduser-marker-check").await;
    assert_eq!(before["rows_walked"], 5, "{before}");
    // five unread, one starred, one trashed with its time
    assert_eq!(before["mismatches"], 8, "{before}");

    let first = post(
        &state,
        "/v1/admin/maintenance:rewrite-threaduser-markers?offset=0&limit=3",
    )
    .await;
    assert_eq!(first["total_rows"], 5, "{first}");
    assert_eq!(first["walked"], 3, "{first}");
    assert_eq!(first["changed"], 3, "{first}");
    assert_eq!(first["done"], false, "{first}");
    let rest = post(
        &state,
        "/v1/admin/maintenance:rewrite-threaduser-markers?offset=3&limit=3",
    )
    .await;
    assert_eq!(rest["walked"], 2, "{rest}");
    assert_eq!(rest["changed"], 2, "{rest}");
    assert_eq!(rest["done"], true, "{rest}");

    let again = post(&state, "/v1/admin/maintenance:rewrite-threaduser-markers").await;
    assert_eq!(again["walked"], 5, "it must still be reading them: {again}");
    assert_eq!(again["changed"], 0, "a second pass writes nothing: {again}");
    assert_eq!(again["already_correct"], 5, "{again}");

    let after = post(&state, "/v1/admin/maintenance:threaduser-marker-check").await;
    assert_eq!(after["mismatches"], 0, "{after}");
    let unread = after["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .find(|c| c["column"] == "unread")
        .expect("unread");
    assert_eq!(unread["rows_set"], 5, "{after}");
    assert_eq!(unread["markers_present"], 5, "{after}");
    assert_eq!(unread["index_entries"], 5, "{after}");
}
