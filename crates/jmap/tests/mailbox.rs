//! Protocol-level integration tests for `Mailbox/get` and `Mailbox/query`
//! (RFC 8621 §2).

use mailrs_jmap::dispatch::dispatch_method;
use mailrs_jmap::fixtures::{EXAMPLE_USER, InMemoryStore, make_message};
use mailrs_jmap::types::{FLAG_SEEN, MailboxCounts};
use serde_json::json;

#[tokio::test]
async fn mailbox_get_all_returns_every_mailbox_with_role_mapping() {
    let store = InMemoryStore::new()
        .with_mailbox(1, "INBOX")
        .with_mailbox(2, "Sent")
        .with_mailbox(3, "Archive");

    let (name, resp) = dispatch_method("Mailbox/get", &json!({}), EXAMPLE_USER, &store)
        .await
        .expect("Mailbox/get succeeds");

    assert_eq!(name, "Mailbox/get");
    assert_eq!(resp["accountId"], EXAMPLE_USER);
    assert_eq!(resp["state"], "0");
    assert_eq!(resp["notFound"], json!([]));

    let list = resp["list"].as_array().unwrap();
    assert_eq!(list.len(), 3);

    assert_eq!(list[0]["id"], "mb-1");
    assert_eq!(list[0]["name"], "INBOX");
    assert_eq!(list[0]["role"], "inbox");
    assert_eq!(list[1]["role"], "sent");
    assert!(list[2].get("role").is_none(), "custom folder has no role");
}

#[tokio::test]
async fn mailbox_get_by_specific_ids_returns_subset() {
    let store = InMemoryStore::new()
        .with_mailbox(1, "INBOX")
        .with_mailbox(2, "Sent");

    let (_, resp) = dispatch_method(
        "Mailbox/get",
        &json!({"ids": ["mb-2"]}),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    let list = resp["list"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], "mb-2");
    assert_eq!(resp["notFound"], json!([]));
}

#[tokio::test]
async fn mailbox_get_malformed_and_missing_ids_land_in_not_found() {
    let store = InMemoryStore::new().with_mailbox(1, "INBOX");

    let (_, resp) = dispatch_method(
        "Mailbox/get",
        &json!({"ids": ["mb-1", "mb-999", "not-a-mb-id"]}),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    let list = resp["list"].as_array().unwrap();
    assert_eq!(list.len(), 1, "only mb-1 found");

    let not_found = resp["notFound"].as_array().unwrap();
    assert_eq!(not_found.len(), 2);
    assert!(not_found.contains(&json!("mb-999")));
    assert!(not_found.contains(&json!("not-a-mb-id")));
}

#[tokio::test]
async fn mailbox_get_counts_default_to_message_status() {
    let unread = make_message(10, 1, EXAMPLE_USER);
    let mut seen = make_message(11, 1, EXAMPLE_USER);
    seen.flags = FLAG_SEEN;
    let store = InMemoryStore::new()
        .with_mailbox(1, "INBOX")
        .with_message(unread)
        .with_message(seen);

    let (_, resp) = dispatch_method(
        "Mailbox/get",
        &json!({"ids": ["mb-1"]}),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    let mb = &resp["list"][0];
    assert_eq!(mb["totalEmails"], 2);
    assert_eq!(mb["unreadEmails"], 1);
}

#[tokio::test]
async fn mailbox_get_propagates_list_error_as_server_fail() {
    let store = InMemoryStore::new().list_mailboxes_fails("db is down");

    let err = dispatch_method("Mailbox/get", &json!({}), EXAMPLE_USER, &store)
        .await
        .unwrap_err();

    let json_err = err.to_json();
    assert_eq!(json_err["type"], "serverFail");
    assert_eq!(json_err["description"], "db is down");
}

#[tokio::test]
async fn mailbox_get_falls_back_when_status_lookup_errors() {
    let store = InMemoryStore::new()
        .with_mailbox(1, "INBOX")
        .mailbox_status_fails("status lookup unavailable");

    let (_, resp) = dispatch_method("Mailbox/get", &json!({}), EXAMPLE_USER, &store)
        .await
        .unwrap();

    let mb = &resp["list"][0];
    // Per the handler contract: best-effort, falls back to (0, 0).
    assert_eq!(mb["totalEmails"], 0);
    assert_eq!(mb["unreadEmails"], 0);
}

#[tokio::test]
async fn mailbox_get_respects_explicit_counts_override() {
    let store = InMemoryStore::new()
        .with_mailbox(1, "INBOX")
        .with_mailbox_counts(
            1,
            MailboxCounts {
                total: 99,
                unread: 7,
            },
        );

    let (_, resp) = dispatch_method("Mailbox/get", &json!({}), EXAMPLE_USER, &store)
        .await
        .unwrap();

    let mb = &resp["list"][0];
    assert_eq!(mb["totalEmails"], 99);
    assert_eq!(mb["unreadEmails"], 7);
}

#[tokio::test]
async fn mailbox_query_returns_all_ids() {
    let store = InMemoryStore::new()
        .with_mailbox(1, "INBOX")
        .with_mailbox(2, "Sent");

    let (name, resp) = dispatch_method("Mailbox/query", &json!({}), EXAMPLE_USER, &store)
        .await
        .unwrap();

    assert_eq!(name, "Mailbox/query");
    assert_eq!(resp["accountId"], EXAMPLE_USER);
    assert_eq!(resp["queryState"], "0");
    assert_eq!(resp["canCalculateChanges"], false);
    assert_eq!(resp["position"], 0);
    assert_eq!(resp["total"], 2);
    assert_eq!(resp["ids"], json!(["mb-1", "mb-2"]));
}
