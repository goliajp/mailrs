//! Protocol-level integration tests for `Thread/get` (RFC 8621 §3.2).

mod common;

use common::{InMemoryStore, TEST_USER, make_message};
use mailrs_jmap::dispatch::dispatch_method;
use serde_json::json;

#[tokio::test]
async fn thread_get_missing_ids_argument_is_invalid_arguments() {
    let store = InMemoryStore::new();

    let err = dispatch_method("Thread/get", &json!({}), TEST_USER, &store)
        .await
        .unwrap_err();

    assert_eq!(err.to_json()["type"], "invalidArguments");
}

#[tokio::test]
async fn thread_get_returns_email_ids_for_known_thread() {
    let mut m1 = make_message(1, 10, TEST_USER);
    m1.thread_id = "t-A".into();
    let mut m2 = make_message(2, 10, TEST_USER);
    m2.thread_id = "t-A".into();
    let mut m3 = make_message(3, 10, TEST_USER);
    m3.thread_id = "t-B".into();

    let store = InMemoryStore::new()
        .with_mailbox(10, "INBOX")
        .with_message(m1)
        .with_message(m2)
        .with_message(m3);

    let (name, resp) = dispatch_method(
        "Thread/get",
        &json!({"ids": ["t-A"]}),
        TEST_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(name, "Thread/get");
    assert_eq!(resp["accountId"], TEST_USER);
    assert_eq!(resp["state"], "0");
    assert_eq!(resp["notFound"], json!([]));

    let list = resp["list"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], "t-A");
    assert_eq!(list[0]["emailIds"], json!(["msg-1", "msg-2"]));
}

#[tokio::test]
async fn thread_get_empty_thread_lands_in_not_found() {
    let store = InMemoryStore::new();

    let (_, resp) = dispatch_method(
        "Thread/get",
        &json!({"ids": ["t-missing"]}),
        TEST_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["list"].as_array().unwrap().len(), 0);
    assert_eq!(resp["notFound"], json!(["t-missing"]));
}

#[tokio::test]
async fn thread_get_store_error_falls_back_to_empty_thread_marked_not_found() {
    // list_thread_messages errors are swallowed by handler (unwrap_or_default);
    // the thread is treated as empty and surfaces in notFound.
    let store = InMemoryStore::new().list_thread_messages_fails("temporary glitch");

    let (_, resp) = dispatch_method(
        "Thread/get",
        &json!({"ids": ["t-A"]}),
        TEST_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["list"].as_array().unwrap().len(), 0);
    assert_eq!(resp["notFound"], json!(["t-A"]));
}

#[tokio::test]
async fn thread_get_filters_threads_by_user() {
    let mut alice_msg = make_message(1, 10, TEST_USER);
    alice_msg.thread_id = "t-shared".into();
    let mut bob_msg = make_message(2, 10, "bob@example.com");
    bob_msg.user_address = "bob@example.com".into();
    bob_msg.thread_id = "t-shared".into();

    let store = InMemoryStore::new()
        .with_mailbox(10, "INBOX")
        .with_message(alice_msg)
        .with_message(bob_msg);

    let (_, resp) = dispatch_method(
        "Thread/get",
        &json!({"ids": ["t-shared"]}),
        TEST_USER,
        &store,
    )
    .await
    .unwrap();

    let list = resp["list"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["emailIds"], json!(["msg-1"]));
}
