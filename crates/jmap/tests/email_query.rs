//! Protocol-level integration tests for `Email/query` (RFC 8621 §4.4).


use mailrs_jmap::fixtures::{InMemoryStore, EXAMPLE_USER, make_message};
use mailrs_jmap::dispatch::dispatch_method;
use mailrs_jmap::types::{FLAG_FLAGGED, FLAG_SEEN};
use serde_json::json;

/// Build a 4-mailbox / 6-message world used by most filter / sort tests.
fn fixture() -> InMemoryStore {
    let mut m1 = make_message(1, 10, EXAMPLE_USER); // INBOX, oldest
    m1.subject = "Project Alpha kickoff".into();
    m1.sender = "alice@example.com".into();
    m1.internal_date = 1_700_000_001;

    let mut m2 = make_message(2, 10, EXAMPLE_USER); // INBOX
    m2.subject = "Re: project alpha".into();
    m2.sender = "bob@example.com".into();
    m2.flags = FLAG_SEEN;
    m2.internal_date = 1_700_000_002;

    let mut m3 = make_message(3, 10, EXAMPLE_USER); // INBOX, newest in inbox
    m3.subject = "weekly digest".into();
    m3.recipients = "team@example.com".into();
    m3.flags = FLAG_SEEN | FLAG_FLAGGED;
    m3.internal_date = 1_700_000_003;

    let mut m4 = make_message(4, 20, EXAMPLE_USER); // Sent
    m4.subject = "Sent thing".into();
    m4.internal_date = 1_700_000_004;

    InMemoryStore::new()
        .with_mailbox(10, "INBOX")
        .with_mailbox(20, "Sent")
        .with_message(m1)
        .with_message(m2)
        .with_message(m3)
        .with_message(m4)
}

#[tokio::test]
async fn email_query_no_filter_returns_all_sorted_desc_by_default() {
    let store = fixture();

    let (name, resp) = dispatch_method("Email/query", &json!({}), EXAMPLE_USER, &store)
        .await
        .unwrap();

    assert_eq!(name, "Email/query");
    assert_eq!(resp["accountId"], EXAMPLE_USER);
    assert_eq!(resp["queryState"], "0");
    assert_eq!(resp["canCalculateChanges"], false);
    assert_eq!(resp["position"], 0);
    assert_eq!(resp["total"], 4);
    // descending by internal_date: 4, 3, 2, 1
    assert_eq!(resp["ids"], json!(["msg-4", "msg-3", "msg-2", "msg-1"]));
}

#[tokio::test]
async fn email_query_in_mailbox_restricts_to_that_mailbox() {
    let store = fixture();

    let (_, resp) = dispatch_method(
        "Email/query",
        &json!({"filter": {"inMailbox": "mb-20"}}),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["total"], 1);
    assert_eq!(resp["ids"], json!(["msg-4"]));
}

#[tokio::test]
async fn email_query_in_mailbox_nonexistent_returns_empty() {
    let store = fixture();

    let (_, resp) = dispatch_method(
        "Email/query",
        &json!({"filter": {"inMailbox": "mb-999"}}),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["total"], 0);
    assert_eq!(resp["ids"], json!([]));
}

#[tokio::test]
async fn email_query_text_filter_is_case_insensitive_across_subject_sender_recipients() {
    let store = fixture();

    // "ALPHA" hits m1 (subject), m2 (subject)
    let (_, resp) = dispatch_method(
        "Email/query",
        &json!({"filter": {"text": "ALPHA"}}),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();
    assert_eq!(resp["total"], 2);

    // "team@example.com" only on m3 recipients
    let (_, resp) = dispatch_method(
        "Email/query",
        &json!({"filter": {"text": "team@example.com"}}),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();
    assert_eq!(resp["total"], 1);
    assert_eq!(resp["ids"], json!(["msg-3"]));

    // "bob@example.com" only on m2 sender
    let (_, resp) = dispatch_method(
        "Email/query",
        &json!({"filter": {"text": "bob@example.com"}}),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();
    assert_eq!(resp["total"], 1);
    assert_eq!(resp["ids"], json!(["msg-2"]));
}

#[tokio::test]
async fn email_query_has_keyword_filters_to_messages_with_that_flag() {
    let store = fixture();

    let (_, resp) = dispatch_method(
        "Email/query",
        &json!({"filter": {"hasKeyword": "$seen"}}),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    // m2 and m3 have FLAG_SEEN
    let ids = resp["ids"].as_array().unwrap();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&json!("msg-2")));
    assert!(ids.contains(&json!("msg-3")));
}

#[tokio::test]
async fn email_query_not_keyword_filters_out_messages_with_that_flag() {
    let store = fixture();

    let (_, resp) = dispatch_method(
        "Email/query",
        &json!({"filter": {"notKeyword": "$seen"}}),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    let ids = resp["ids"].as_array().unwrap();
    // m1 and m4 lack FLAG_SEEN
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&json!("msg-1")));
    assert!(ids.contains(&json!("msg-4")));
}

#[tokio::test]
async fn email_query_sort_ascending_flips_order() {
    let store = fixture();

    let (_, resp) = dispatch_method(
        "Email/query",
        &json!({"sort": [{"property": "receivedAt", "isAscending": true}]}),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["ids"], json!(["msg-1", "msg-2", "msg-3", "msg-4"]));
}

#[tokio::test]
async fn email_query_position_and_limit_paginate() {
    let store = fixture();

    let (_, resp) = dispatch_method(
        "Email/query",
        &json!({"position": 1, "limit": 2}),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["total"], 4);
    assert_eq!(resp["position"], 1);
    // desc order: [4, 3, 2, 1] → skip 1, take 2 = [3, 2]
    assert_eq!(resp["ids"], json!(["msg-3", "msg-2"]));
}

#[tokio::test]
async fn email_query_limit_clamped_to_500() {
    let store = fixture();

    let (_, resp) = dispatch_method(
        "Email/query",
        &json!({"limit": 100_000}),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    // total is 4, limit clamp doesn't affect ids array length here, but the
    // handler must not panic on huge limit values.
    assert_eq!(resp["total"], 4);
    assert_eq!(resp["ids"].as_array().unwrap().len(), 4);
}

#[tokio::test]
async fn email_query_propagates_list_mailboxes_error_as_server_fail() {
    let store = InMemoryStore::new().list_mailboxes_fails("mailboxes unavailable");

    let err = dispatch_method("Email/query", &json!({}), EXAMPLE_USER, &store)
        .await
        .unwrap_err();

    let v = err.to_json();
    assert_eq!(v["type"], "serverFail");
    assert_eq!(v["description"], "mailboxes unavailable");
}
