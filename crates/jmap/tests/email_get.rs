//! Protocol-level integration tests for `Email/get` (RFC 8621 §4.2).


use mailrs_jmap::fixtures::{InMemoryStore, EXAMPLE_USER, make_message, parsed_with_attachment, parsed_with_text};
use mailrs_jmap::dispatch::dispatch_method;
use mailrs_jmap::types::FLAG_SEEN;
use serde_json::json;

#[tokio::test]
async fn email_get_missing_ids_argument_is_invalid_arguments() {
    let store = InMemoryStore::new();

    let err = dispatch_method("Email/get", &json!({}), EXAMPLE_USER, &store)
        .await
        .unwrap_err();

    assert_eq!(err.to_json()["type"], "invalidArguments");
}

#[tokio::test]
async fn email_get_returns_full_metadata_when_properties_omitted() {
    let mut msg = make_message(42, 1, EXAMPLE_USER);
    msg.flags = FLAG_SEEN;
    msg.subject = "hello".into();
    let store = InMemoryStore::new()
        .with_mailbox(1, "INBOX")
        .with_message(msg);

    let (_, resp) = dispatch_method(
        "Email/get",
        &json!({"ids": ["msg-42"]}),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    let list = resp["list"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    let m = &list[0];
    assert_eq!(m["id"], "msg-42");
    assert_eq!(m["subject"], "hello");
    assert_eq!(m["threadId"], "thread-42");
    assert_eq!(m["mailboxIds"]["mb-1"], true);
    assert_eq!(m["keywords"]["$seen"], true);
    // body fields present (defaulted, since no raw bytes set)
    assert!(m.get("bodyValues").is_some());
    assert_eq!(m["textBody"], json!([]));
}

#[tokio::test]
async fn email_get_malformed_and_missing_ids_land_in_not_found() {
    let store = InMemoryStore::new()
        .with_mailbox(1, "INBOX")
        .with_message(make_message(1, 1, EXAMPLE_USER));

    let (_, resp) = dispatch_method(
        "Email/get",
        &json!({"ids": ["msg-1", "msg-999", "not-an-email"]}),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["list"].as_array().unwrap().len(), 1);
    let not_found = resp["notFound"].as_array().unwrap();
    assert_eq!(not_found.len(), 2);
    assert!(not_found.contains(&json!("msg-999")));
    assert!(not_found.contains(&json!("not-an-email")));
}

#[tokio::test]
async fn email_get_unowned_message_is_not_found() {
    let mut msg = make_message(1, 1, "bob@example.com");
    msg.user_address = "bob@example.com".into();
    let store = InMemoryStore::new()
        .with_mailbox(1, "INBOX")
        .with_message(msg);

    let (_, resp) = dispatch_method(
        "Email/get",
        &json!({"ids": ["msg-1"]}),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    assert_eq!(resp["list"].as_array().unwrap().len(), 0);
    assert_eq!(resp["notFound"], json!(["msg-1"]));
}

#[tokio::test]
async fn email_get_skips_body_read_when_only_metadata_properties_requested() {
    let store = InMemoryStore::new()
        .with_mailbox(1, "INBOX")
        .with_message(make_message(1, 1, EXAMPLE_USER));

    let (_, resp) = dispatch_method(
        "Email/get",
        &json!({"ids": ["msg-1"], "properties": ["subject", "from"]}),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    let m = &resp["list"][0];
    assert!(m.get("bodyValues").is_none(), "bodyValues skipped");
    assert!(m.get("textBody").is_none());
    assert!(m.get("htmlBody").is_none());
    assert!(m.get("attachments").is_none());
}

#[tokio::test]
async fn email_get_populates_body_when_bodyvalues_requested() {
    let raw = b"From: x\r\n\r\nbody payload".to_vec();
    let store = InMemoryStore::new()
        .with_mailbox(1, "INBOX")
        .with_message(make_message(1, 1, EXAMPLE_USER))
        .with_message_raw(1, raw.clone())
        .with_parsed_body(raw, parsed_with_text("body payload"));

    let (_, resp) = dispatch_method(
        "Email/get",
        &json!({"ids": ["msg-1"], "properties": ["bodyValues", "textBody"]}),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    let m = &resp["list"][0];
    assert_eq!(m["bodyValues"]["t1"]["value"], "body payload");
    assert_eq!(m["textBody"][0]["partId"], "t1");
}

#[tokio::test]
async fn email_get_emits_attachment_metadata_when_requested() {
    let raw = b"raw with attachment".to_vec();
    let store = InMemoryStore::new()
        .with_mailbox(1, "INBOX")
        .with_message(make_message(1, 1, EXAMPLE_USER))
        .with_message_raw(1, raw.clone())
        .with_parsed_body(raw, parsed_with_attachment("report.pdf", "application/pdf", 1024));

    let (_, resp) = dispatch_method(
        "Email/get",
        &json!({"ids": ["msg-1"], "properties": ["attachments", "hasAttachment"]}),
        EXAMPLE_USER,
        &store,
    )
    .await
    .unwrap();

    let m = &resp["list"][0];
    assert_eq!(m["hasAttachment"], true);
    let att = m["attachments"].as_array().unwrap();
    assert_eq!(att.len(), 1);
    assert_eq!(att[0]["name"], "report.pdf");
    assert_eq!(att[0]["type"], "application/pdf");
    assert_eq!(att[0]["size"], 1024);
}
