//! Integration tests for the [`dispatch_request`] envelope: ordering,
//! back-references (RFC 8620 §3.7), unknown-method handling, and the wrapping
//! `JmapResponse` shape.


use mailrs_jmap::fixtures::{InMemoryStore, EXAMPLE_USER, make_message, make_request};
use mailrs_jmap::dispatch::{JmapRequest, dispatch_request};
use serde_json::json;

#[tokio::test]
async fn dispatch_request_empty_method_calls_yields_empty_responses() {
    let store = InMemoryStore::new();

    let req = JmapRequest {
        using: vec!["urn:ietf:params:jmap:mail".into()],
        method_calls: vec![],
    };
    let resp = dispatch_request(req, EXAMPLE_USER, &store).await;

    assert!(resp.method_responses.is_empty());
    assert_eq!(resp.session_state, "0");
}

#[tokio::test]
async fn dispatch_request_unknown_method_surfaces_as_error_envelope_at_position() {
    let store = InMemoryStore::new().with_mailbox(1, "INBOX");

    let req = make_request(&[
        ("Mailbox/get", json!({}), "c1"),
        ("Bogus/method", json!({}), "c2"),
    ]);
    let resp = dispatch_request(req, EXAMPLE_USER, &store).await;

    assert_eq!(resp.method_responses.len(), 2);
    assert_eq!(resp.method_responses[0].0, "Mailbox/get");
    assert_eq!(resp.method_responses[1].0, "error");
    assert_eq!(resp.method_responses[1].1["type"], "unknownMethod");
    assert_eq!(resp.method_responses[1].2, "c2", "call_id preserved");
}

#[tokio::test]
async fn dispatch_request_single_mailbox_get_returns_full_envelope() {
    let store = InMemoryStore::new().with_mailbox(1, "INBOX");

    let req = make_request(&[("Mailbox/get", json!({}), "c1")]);
    let resp = dispatch_request(req, EXAMPLE_USER, &store).await;

    assert_eq!(resp.method_responses.len(), 1);
    let (name, value, call_id) = &resp.method_responses[0];
    assert_eq!(name, "Mailbox/get");
    assert_eq!(call_id, "c1");
    assert_eq!(value["accountId"], EXAMPLE_USER);
    assert_eq!(value["list"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn dispatch_request_back_reference_resolves_ids_from_prior_query() {
    let store = InMemoryStore::new()
        .with_mailbox(10, "INBOX")
        .with_message(make_message(1, 10, EXAMPLE_USER))
        .with_message(make_message(2, 10, EXAMPLE_USER));

    // Email/query → Email/get with #ids back-reference to /ids of the query.
    let req = make_request(&[
        ("Email/query", json!({}), "c1"),
        (
            "Email/get",
            json!({
                "#ids": {"resultOf": "c1", "name": "Email/query", "path": "/ids"},
                "properties": ["subject"]
            }),
            "c2",
        ),
    ]);
    let resp = dispatch_request(req, EXAMPLE_USER, &store).await;

    assert_eq!(resp.method_responses.len(), 2);
    assert_eq!(resp.method_responses[1].0, "Email/get");
    let list = resp.method_responses[1].1["list"].as_array().unwrap();
    assert_eq!(list.len(), 2, "both queried messages got loaded");
}

#[tokio::test]
async fn dispatch_request_back_reference_to_unknown_call_drops_arg() {
    // When the back-ref target call_id doesn't exist, the ref key is stripped
    // and no replacement is inserted. Email/get then sees no `ids` field and
    // returns InvalidArguments.
    let store = InMemoryStore::new();

    let req = make_request(&[(
        "Email/get",
        json!({
            "#ids": {"resultOf": "nope", "name": "Email/query", "path": "/ids"}
        }),
        "c1",
    )]);
    let resp = dispatch_request(req, EXAMPLE_USER, &store).await;

    assert_eq!(resp.method_responses[0].0, "error");
    assert_eq!(resp.method_responses[0].1["type"], "invalidArguments");
}

#[tokio::test]
async fn dispatch_request_preserves_method_call_order_in_responses() {
    let store = InMemoryStore::new().with_mailbox(1, "INBOX");

    let req = make_request(&[
        ("Mailbox/get", json!({}), "first"),
        ("Mailbox/query", json!({}), "second"),
        ("Mailbox/get", json!({}), "third"),
    ]);
    let resp = dispatch_request(req, EXAMPLE_USER, &store).await;

    let call_ids: Vec<&str> = resp
        .method_responses
        .iter()
        .map(|(_, _, c)| c.as_str())
        .collect();
    assert_eq!(call_ids, vec!["first", "second", "third"]);
}

#[tokio::test]
async fn dispatch_request_method_level_error_uses_canonical_envelope_shape() {
    // ServerFail must come back as `("error", {type, description}, call_id)`.
    let store = InMemoryStore::new().list_mailboxes_fails("explode");

    let req = make_request(&[("Mailbox/get", json!({}), "c1")]);
    let resp = dispatch_request(req, EXAMPLE_USER, &store).await;

    let (name, value, call_id) = &resp.method_responses[0];
    assert_eq!(name, "error");
    assert_eq!(value["type"], "serverFail");
    assert_eq!(value["description"], "explode");
    assert_eq!(call_id, "c1");
}
