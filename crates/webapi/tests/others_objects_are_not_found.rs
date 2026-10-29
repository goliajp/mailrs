//! An identifier names an object only inside its owner's account. Asking
//! for somebody else's is answered as if it did not exist.

mod live;

use reqwest::Method;

const KEVY: &str = "mem://webapi-others-objects-are-not-found";
const ALICE: &str = "alice@example.test";
const BOB: &str = "bob@example.test";

#[tokio::test]
async fn another_accounts_objects_are_not_found() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    // alice first: the old JMAP lookup answered from the first account
    live.add_account(ALICE, "pw").await;
    live.add_account(BOB, "pw").await;
    let alice = live.login(ALICE, "pw").await;
    let bob = live.login(BOB, "pw").await;

    jmap_mailboxes_are_the_callers(&live, &alice, &bob).await;
    webhooks_are_deleted_only_by_their_owner(&live, &alice, &bob).await;
}

/// Mailbox ids are the same small numbers in every account, so a JMAP
/// mailbox id must be read in the caller's account.
async fn jmap_mailboxes_are_the_callers(live: &live::Live, alice: &str, bob: &str) {
    live.mailbox.ensure_thread_table();
    live.mailbox
        .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
            category: "inbox",
            is_own: false,
            latest_date: 1_786_000_000,
            latest_preview: "for alice only",
            senders_csv: "Friend <friend@partner.example>",
            subject: "for alice only",
            thread_id: "t-alice",
            unread: true,
            user: ALICE,
        })
        .expect("arrival");
    let unread = |token: String| async move {
        let req = serde_json::json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
            "methodCalls": [["Mailbox/get", { "ids": null }, "c1"]],
        });
        let (status, body) = live.call(Method::POST, "/jmap", &token, Some(req)).await;
        assert_eq!(status, 200, "{body}");
        body["methodResponses"][0][1]["list"]
            .as_array()
            .expect("list")
            .iter()
            .find(|m| {
                m["name"]
                    .as_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case("INBOX"))
            })
            .map(|m| m["unreadEmails"].as_u64().unwrap_or(0))
            .expect("an inbox")
    };
    assert_eq!(unread(alice.to_string()).await, 1, "alice sees her own");
    assert_eq!(unread(bob.to_string()).await, 0, "bob sees only his");
}

/// Webhook ids come from one counter for everybody; another account's id
/// is not found.
async fn webhooks_are_deleted_only_by_their_owner(live: &live::Live, alice: &str, bob: &str) {
    let (status, created) = live
        .call(
            Method::POST,
            "/api/agent/webhooks",
            alice,
            Some(serde_json::json!({ "url": "https://hooks.example/a", "event_type": "new_mail" })),
        )
        .await;
    assert!((200..300).contains(&status), "create: {status} {created}");
    let id = created["id"].as_i64().expect("id");
    let path = format!("/api/agent/webhooks/{id}");

    let (status, _) = live.call(Method::DELETE, &path, bob, None).await;
    assert_eq!(status, 404, "bob deleting alice's webhook");
    let (_, list) = live
        .call(Method::GET, "/api/agent/webhooks", alice, None)
        .await;
    assert!(
        list.to_string().contains("hooks.example/a"),
        "alice's webhook survived: {list}"
    );

    let (status, _) = live.call(Method::DELETE, &path, alice, None).await;
    assert_eq!(status, 204, "alice deleting her own");
}
