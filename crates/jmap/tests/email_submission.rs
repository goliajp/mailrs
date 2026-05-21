//! Protocol-level integration tests for `EmailSubmission/set` (RFC 8621 §7.3).
//!
//! The PRIORITY surface for outbound flow: create-only semantics, every error
//! path, and the success-shape carrying the synthetic `sub-{id}` identifier.

mod common;

use common::{InMemoryStore, TEST_USER, make_message};
use mailrs_jmap::dispatch::dispatch_method;
use serde_json::json;

#[tokio::test]
async fn submission_set_no_create_returns_empty_state_zero() {
    let store = InMemoryStore::new();

    let (name, resp) = dispatch_method(
        "EmailSubmission/set",
        &json!({}),
        TEST_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(name, "EmailSubmission/set");
    assert_eq!(resp["accountId"], TEST_USER);
    assert_eq!(resp["oldState"], "0");
    assert_eq!(resp["newState"], "0", "newState stays at 0 when nothing happened");
    assert_eq!(resp["created"], json!({}));
    assert_eq!(resp["notCreated"], json!({}));
}

#[tokio::test]
async fn submission_set_response_shape_advances_state_when_create_present() {
    let store = InMemoryStore::new();

    let (_, resp) = dispatch_method(
        "EmailSubmission/set",
        &json!({"create": {}}),
        TEST_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["oldState"], "0");
    assert_eq!(resp["newState"], "1");
}

#[tokio::test]
async fn submission_set_create_success_returns_synthetic_sub_id() {
    let raw = b"From: alice\r\n\r\nbody".to_vec();
    let store = InMemoryStore::new()
        .with_mailbox(20, "Drafts")
        .with_message(make_message(42, 20, TEST_USER))
        .with_message_raw(42, raw);

    let (_, resp) = dispatch_method(
        "EmailSubmission/set",
        &json!({
            "create": {"k1": {"emailId": "msg-42"}}
        }),
        TEST_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["notCreated"], json!({}));
    assert_eq!(resp["created"]["k1"]["id"], "sub-42");
    assert_eq!(resp["created"]["k1"]["emailId"], "msg-42");
    assert_eq!(resp["created"]["k1"]["undoStatus"], "final");
}

#[tokio::test]
async fn submission_set_missing_email_id_lands_in_invalid_properties() {
    let store = InMemoryStore::new();

    let (_, resp) = dispatch_method(
        "EmailSubmission/set",
        &json!({
            "create": {"k1": {}}
        }),
        TEST_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["created"], json!({}));
    assert_eq!(resp["notCreated"]["k1"]["type"], "invalidProperties");
    assert_eq!(resp["notCreated"]["k1"]["description"], "emailId is required");
}

#[tokio::test]
async fn submission_set_malformed_email_id_lands_in_invalid_properties() {
    let store = InMemoryStore::new();

    let (_, resp) = dispatch_method(
        "EmailSubmission/set",
        &json!({
            "create": {"k1": {"emailId": "not-an-email"}}
        }),
        TEST_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["notCreated"]["k1"]["type"], "invalidProperties");
    assert_eq!(resp["notCreated"]["k1"]["description"], "invalid emailId");
}

#[tokio::test]
async fn submission_set_missing_message_lands_in_not_found() {
    let store = InMemoryStore::new();

    let (_, resp) = dispatch_method(
        "EmailSubmission/set",
        &json!({
            "create": {"k1": {"emailId": "msg-999"}}
        }),
        TEST_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["notCreated"]["k1"]["type"], "notFound");
    assert_eq!(resp["notCreated"]["k1"]["description"], "email not found");
}

#[tokio::test]
async fn submission_set_unowned_message_lands_in_not_found() {
    let mut msg = make_message(42, 20, "bob@example.com");
    msg.user_address = "bob@example.com".into();
    let store = InMemoryStore::new()
        .with_mailbox(20, "Drafts")
        .with_message(msg);

    let (_, resp) = dispatch_method(
        "EmailSubmission/set",
        &json!({
            "create": {"k1": {"emailId": "msg-42"}}
        }),
        TEST_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["notCreated"]["k1"]["type"], "notFound");
}

#[tokio::test]
async fn submission_set_no_raw_bytes_lands_in_server_fail() {
    // Message exists but read_message_raw returns None (no raw_bytes set).
    let store = InMemoryStore::new()
        .with_mailbox(20, "Drafts")
        .with_message(make_message(42, 20, TEST_USER));

    let (_, resp) = dispatch_method(
        "EmailSubmission/set",
        &json!({
            "create": {"k1": {"emailId": "msg-42"}}
        }),
        TEST_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["notCreated"]["k1"]["type"], "serverFail");
    assert_eq!(
        resp["notCreated"]["k1"]["description"],
        "could not read message"
    );
}

#[tokio::test]
async fn submission_set_store_failure_carries_message_into_description() {
    let raw = b"body".to_vec();
    let store = InMemoryStore::new()
        .with_mailbox(20, "Drafts")
        .with_message(make_message(42, 20, TEST_USER))
        .with_message_raw(42, raw)
        .submission_fails_with("MX bounced");

    let (_, resp) = dispatch_method(
        "EmailSubmission/set",
        &json!({
            "create": {"k1": {"emailId": "msg-42"}}
        }),
        TEST_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["notCreated"]["k1"]["type"], "serverFail");
    assert_eq!(resp["notCreated"]["k1"]["description"], "MX bounced");
}

#[tokio::test]
async fn submission_set_silent_failure_uses_default_delivery_failed_message() {
    let raw = b"body".to_vec();
    let store = InMemoryStore::new()
        .with_mailbox(20, "Drafts")
        .with_message(make_message(42, 20, TEST_USER))
        .with_message_raw(42, raw)
        .submission_fails_silently();

    let (_, resp) = dispatch_method(
        "EmailSubmission/set",
        &json!({
            "create": {"k1": {"emailId": "msg-42"}}
        }),
        TEST_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["notCreated"]["k1"]["type"], "serverFail");
    assert_eq!(resp["notCreated"]["k1"]["description"], "delivery failed");
}

#[tokio::test]
async fn submission_set_mixes_success_and_failure_across_creates() {
    let raw = b"body".to_vec();
    let store = InMemoryStore::new()
        .with_mailbox(20, "Drafts")
        .with_message(make_message(1, 20, TEST_USER))
        .with_message_raw(1, raw);

    let (_, resp) = dispatch_method(
        "EmailSubmission/set",
        &json!({
            "create": {
                "ok": {"emailId": "msg-1"},
                "bad": {"emailId": "msg-999"},
                "noid": {}
            }
        }),
        TEST_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["created"]["ok"]["id"], "sub-1");
    assert_eq!(resp["notCreated"]["bad"]["type"], "notFound");
    assert_eq!(resp["notCreated"]["noid"]["type"], "invalidProperties");
}
