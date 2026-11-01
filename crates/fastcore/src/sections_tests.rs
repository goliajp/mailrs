use axum::body::Body;
use axum::http::{Method, Request};
use tower::ServiceExt;

use crate::build_router;
use crate::tests::fresh_state;

async fn call(
    app: axum::Router,
    method: Method,
    uri: &str,
    body: serde_json::Value,
) -> serde_json::Value {
    let resp = app
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("Content-Type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    assert!(
        status.is_success(),
        "{uri}: {status} {}",
        String::from_utf8_lossy(&bytes)
    );
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}

/// A message delivered with its facts lists with its section, on the
/// ordinary list and on the section's own.
#[tokio::test]
async fn a_delivered_message_lists_with_its_section() {
    let state = fresh_state();
    let app = build_router(state);
    let wire = serde_json::json!({
        "id": 0, "mailbox_id": 0, "uid": 1, "blob_ref": "1.host",
        "sender": "Ann <ann@y.test>", "recipients": "u@x.test", "subject": "Hi",
        "date": 100, "internal_date": 100, "size": 1, "flags": 0,
        "message_id": "m1@y.test", "in_reply_to": "", "thread_id": "t1", "modseq": 0,
    });
    let req = serde_json::json!({
        "message_id": "m1@y.test", "subject": "Hi", "senders_csv": "Ann <ann@y.test>",
        "latest_date": 100, "latest_preview": "p", "category": "inbox", "unread": true,
        "uid": 1, "payload_wire_json": wire.to_string(),
        "section_facts": {"direct": true, "from_person": true, "first_contact": false, "to_others": false},
    });
    call(
        app.clone(),
        Method::POST,
        "/v1/users/u@x.test/threads/t1/messages",
        req,
    )
    .await;
    let all = call(
        app.clone(),
        Method::POST,
        "/v1/users/u@x.test/conversations:list",
        serde_json::json!({"limit": 10}),
    )
    .await;
    assert_eq!(all["items"][0]["section"], "reply", "{all}");
    let reply = call(
        app,
        Method::POST,
        "/v1/users/u@x.test/conversations:list",
        serde_json::json!({"limit": 10, "section": "reply"}),
    )
    .await;
    assert_eq!(reply["items"][0]["thread_id"], "t1", "{reply}");
}
