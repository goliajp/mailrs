//! Protocol-level integration tests for `Email/set` (RFC 8621 §4.6).
//!
//! The PRIORITY surface for this crate: update / destroy semantics across the
//! full keyword-replace + patch dialects + every documented error path.


use mailrs_jmap::fixtures::{InMemoryStore, EXAMPLE_USER, make_message};
use mailrs_jmap::dispatch::dispatch_method;
use mailrs_jmap::types::{FLAG_ANSWERED, FLAG_DELETED, FLAG_FLAGGED, FLAG_SEEN};
use serde_json::json;

#[tokio::test]
async fn email_set_response_shape() {
    let store = InMemoryStore::new();

    let (name, resp) = dispatch_method("Email/set", &json!({}), EXAMPLE_USER, &store)
        .await
        .unwrap();

    assert_eq!(name, "Email/set");
    assert_eq!(resp["accountId"], EXAMPLE_USER);
    assert_eq!(resp["oldState"], "0");
    assert_eq!(resp["newState"], "1");
    assert_eq!(resp["updated"], json!({}));
    assert_eq!(resp["destroyed"], json!([]));
    assert_eq!(resp["notUpdated"], json!({}));
    assert_eq!(resp["notDestroyed"], json!({}));
}

#[tokio::test]
async fn email_set_update_full_keywords_replaces_flag_bitmask() {
    let mut msg = make_message(1, 10, EXAMPLE_USER);
    msg.flags = FLAG_SEEN | FLAG_ANSWERED;
    let store = InMemoryStore::new()
        .with_mailbox(10, "INBOX")
        .with_message(msg);

    let (_, resp) = dispatch_method(
        "Email/set",
        &json!({
            "update": {
                "msg-1": {"keywords": {"$flagged": true}}
            }
        }),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["updated"]["msg-1"], json!(null));
    assert_eq!(resp["notUpdated"], json!({}));
    assert_eq!(store.flags_for(10, 1), Some(FLAG_FLAGGED));
}

#[tokio::test]
async fn email_set_update_patch_or_in_seen_bit() {
    let msg = make_message(1, 10, EXAMPLE_USER); // flags = 0
    let store = InMemoryStore::new()
        .with_mailbox(10, "INBOX")
        .with_message(msg);

    let (_, resp) = dispatch_method(
        "Email/set",
        &json!({
            "update": {"msg-1": {"keywords/$seen": true}}
        }),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["updated"]["msg-1"], json!(null));
    assert_eq!(store.flags_for(10, 1), Some(FLAG_SEEN));
}

#[tokio::test]
async fn email_set_update_patch_clears_seen_bit() {
    let mut msg = make_message(1, 10, EXAMPLE_USER);
    msg.flags = FLAG_SEEN | FLAG_FLAGGED;
    let store = InMemoryStore::new()
        .with_mailbox(10, "INBOX")
        .with_message(msg);

    let (_, resp) = dispatch_method(
        "Email/set",
        &json!({
            "update": {"msg-1": {"keywords/$seen": false}}
        }),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["updated"]["msg-1"], json!(null));
    // $flagged remains, $seen cleared
    assert_eq!(store.flags_for(10, 1), Some(FLAG_FLAGGED));
}

#[tokio::test]
async fn email_set_update_patch_applies_multiple_keys_in_one_call() {
    let msg = make_message(1, 10, EXAMPLE_USER);
    let store = InMemoryStore::new()
        .with_mailbox(10, "INBOX")
        .with_message(msg);

    let (_, resp) = dispatch_method(
        "Email/set",
        &json!({
            "update": {"msg-1": {
                "keywords/$seen": true,
                "keywords/$answered": true,
                "keywords/$flagged": false
            }}
        }),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["updated"]["msg-1"], json!(null));
    assert_eq!(
        store.flags_for(10, 1),
        Some(FLAG_SEEN | FLAG_ANSWERED)
    );
}

#[tokio::test]
async fn email_set_update_malformed_id_lands_in_not_updated() {
    let store = InMemoryStore::new()
        .with_mailbox(10, "INBOX")
        .with_message(make_message(1, 10, EXAMPLE_USER));

    let (_, resp) = dispatch_method(
        "Email/set",
        &json!({
            "update": {"not-a-msg": {"keywords/$seen": true}}
        }),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["updated"], json!({}));
    assert_eq!(resp["notUpdated"]["not-a-msg"]["type"], "notFound");
}

#[tokio::test]
async fn email_set_update_missing_message_lands_in_not_updated() {
    let store = InMemoryStore::new().with_mailbox(10, "INBOX");

    let (_, resp) = dispatch_method(
        "Email/set",
        &json!({
            "update": {"msg-999": {"keywords/$seen": true}}
        }),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["notUpdated"]["msg-999"]["type"], "notFound");
}

#[tokio::test]
async fn email_set_update_unowned_message_lands_in_not_updated() {
    let mut msg = make_message(1, 10, "bob@example.com");
    msg.user_address = "bob@example.com".into();
    let store = InMemoryStore::new()
        .with_mailbox(10, "INBOX")
        .with_message(msg);

    let (_, resp) = dispatch_method(
        "Email/set",
        &json!({
            "update": {"msg-1": {"keywords/$seen": true}}
        }),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["notUpdated"]["msg-1"]["type"], "notFound");
}

#[tokio::test]
async fn email_set_update_propagates_store_error_as_server_fail_per_entry() {
    let store = InMemoryStore::new()
        .with_mailbox(10, "INBOX")
        .with_message(make_message(1, 10, EXAMPLE_USER))
        .update_flags_fails("disk full");

    let (_, resp) = dispatch_method(
        "Email/set",
        &json!({
            "update": {"msg-1": {"keywords/$seen": true}}
        }),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["updated"], json!({}));
    assert_eq!(resp["notUpdated"]["msg-1"]["type"], "serverFail");
    assert_eq!(resp["notUpdated"]["msg-1"]["description"], "disk full");
}

#[tokio::test]
async fn email_set_destroy_adds_deleted_flag_and_lists_id() {
    let store = InMemoryStore::new()
        .with_mailbox(10, "INBOX")
        .with_message(make_message(1, 10, EXAMPLE_USER));

    let (_, resp) = dispatch_method(
        "Email/set",
        &json!({"destroy": ["msg-1"]}),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["destroyed"], json!(["msg-1"]));
    assert_eq!(resp["notDestroyed"], json!({}));
    assert_eq!(
        store.flags_for(10, 1).unwrap() & FLAG_DELETED,
        FLAG_DELETED
    );
}

#[tokio::test]
async fn email_set_destroy_malformed_id_lands_in_not_destroyed() {
    let store = InMemoryStore::new().with_mailbox(10, "INBOX");

    let (_, resp) = dispatch_method(
        "Email/set",
        &json!({"destroy": ["not-a-msg"]}),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["destroyed"], json!([]));
    assert_eq!(resp["notDestroyed"]["not-a-msg"]["type"], "notFound");
}

#[tokio::test]
async fn email_set_destroy_propagates_store_error_as_server_fail_per_entry() {
    let store = InMemoryStore::new()
        .with_mailbox(10, "INBOX")
        .with_message(make_message(1, 10, EXAMPLE_USER))
        .add_flags_fails("queue full");

    let (_, resp) = dispatch_method(
        "Email/set",
        &json!({"destroy": ["msg-1"]}),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["destroyed"], json!([]));
    assert_eq!(resp["notDestroyed"]["msg-1"]["type"], "serverFail");
    assert_eq!(resp["notDestroyed"]["msg-1"]["description"], "queue full");
}

#[tokio::test]
async fn email_set_update_and_destroy_in_same_call_both_apply() {
    let store = InMemoryStore::new()
        .with_mailbox(10, "INBOX")
        .with_message(make_message(1, 10, EXAMPLE_USER))
        .with_message(make_message(2, 10, EXAMPLE_USER));

    let (_, resp) = dispatch_method(
        "Email/set",
        &json!({
            "update": {"msg-1": {"keywords/$seen": true}},
            "destroy": ["msg-2"]
        }),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["updated"]["msg-1"], json!(null));
    assert_eq!(resp["destroyed"], json!(["msg-2"]));
    assert_eq!(store.flags_for(10, 1), Some(FLAG_SEEN));
    assert_eq!(store.flags_for(10, 2).unwrap() & FLAG_DELETED, FLAG_DELETED);
}

#[tokio::test]
async fn email_set_partial_failure_keeps_succeeding_entries_in_updated() {
    // Two updates: msg-1 exists, msg-2 doesn't. Expect msg-1 in updated,
    // msg-2 in notUpdated.
    let store = InMemoryStore::new()
        .with_mailbox(10, "INBOX")
        .with_message(make_message(1, 10, EXAMPLE_USER));

    let (_, resp) = dispatch_method(
        "Email/set",
        &json!({
            "update": {
                "msg-1": {"keywords/$seen": true},
                "msg-2": {"keywords/$seen": true}
            }
        }),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["updated"]["msg-1"], json!(null));
    assert_eq!(resp["notUpdated"]["msg-2"]["type"], "notFound");
}
