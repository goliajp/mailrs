//! Deleting through the web moves a conversation to the Trash: it leaves
//! its list, the Trash lists it, and restoring puts it back. Deleting
//! for good and emptying the Trash need a recent password confirmation,
//! and so does MCP's way of deleting for good.

mod live;

use reqwest::Method;
use serde_json::{Value, json};

const KEVY: &str = "mem://webapi-the-trash-through-the-web";
const ALICE: &str = "alice@example.test";
const PW: &str = "pw";

fn ids(list: &Value) -> Vec<String> {
    list.as_array()
        .expect("a list")
        .iter()
        .map(|r| r["thread_id"].as_str().expect("thread_id").to_string())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deleting_goes_to_the_trash_and_for_good_needs_a_password() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ALICE, PW).await;
    let alice = live.login(ALICE, PW).await;
    live.mailbox.ensure_thread_table();
    for (tid, date) in [("t-1", 1_786_000_000), ("t-2", 1_786_000_100)] {
        live.mailbox
            .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
                category: "inbox",
                is_own: false,
                latest_date: date,
                latest_preview: "hello",
                senders_csv: "friend@partner.example",
                subject: "hello",
                thread_id: tid,
                unread: true,
                user: ALICE,
            })
            .expect("arrival");
    }
    let list = |q: &'static str| {
        let alice = alice.clone();
        let live = &live;
        async move {
            let (status, body) = live
                .call(
                    Method::GET,
                    &format!("/api/conversations?{q}"),
                    &alice,
                    None,
                )
                .await;
            assert_eq!(status, 200, "{q}: {body}");
            body
        }
    };

    let (status, _) = live
        .call(Method::DELETE, "/api/conversations/t-1", &alice, None)
        .await;
    assert_eq!(status, 204);
    assert_eq!(ids(&list("folder=Inbox").await), ["t-2"]);
    let trash = list("trashed=true").await;
    assert_eq!(ids(&trash), ["t-1"]);
    assert!(trash[0]["trashed_at"].as_i64().unwrap_or(0) > 0, "{trash}");
    assert_eq!(list("folder=Inbox").await[0]["trashed_at"], 0);

    let (status, _) = live
        .call(Method::POST, "/api/conversations/t-1/untrash", &alice, None)
        .await;
    assert_eq!(status, 204);
    assert_eq!(ids(&list("folder=Inbox").await), ["t-2", "t-1"]);
    assert!(ids(&list("trashed=true").await).is_empty());

    // for good: refused until the password is confirmed again
    let (status, body) = live
        .call(
            Method::DELETE,
            "/api/conversations/t-1/forever",
            &alice,
            None,
        )
        .await;
    assert_eq!(status, 403);
    assert_eq!(body["error"], "confirm_required");
    let (status, _) = live
        .call(Method::POST, "/api/trash/empty", &alice, Some(json!({})))
        .await;
    assert_eq!(status, 403);
    let mut mcp = live.mcp(&alice).await;
    let refused = mcp
        .tool("delete_thread_forever", json!({ "thread_id": "t-1" }))
        .await;
    assert!(refused.is_err(), "MCP deleted for good unconfirmed");

    live.confirm(&alice, PW).await;
    let (status, _) = live
        .call(
            Method::DELETE,
            "/api/conversations/t-1/forever",
            &alice,
            None,
        )
        .await;
    assert_eq!(status, 204);
    assert_eq!(ids(&list("folder=Inbox").await), ["t-2"]);
    assert!(ids(&list("trashed=true").await).is_empty());

    // MCP's delete is the Trash too, and restore brings it back
    mcp.tool("delete_thread", json!({ "thread_id": "t-2" }))
        .await
        .expect("delete_thread");
    assert_eq!(ids(&list("trashed=true").await), ["t-2"]);
    mcp.tool("restore_thread", json!({ "thread_id": "t-2" }))
        .await
        .expect("restore_thread");
    assert_eq!(ids(&list("folder=Inbox").await), ["t-2"]);

    let (status, _) = live
        .call(Method::DELETE, "/api/conversations/t-2", &alice, None)
        .await;
    assert_eq!(status, 204);
    let (status, body) = live
        .call(Method::POST, "/api/trash/empty", &alice, Some(json!({})))
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["deleted"], 1);
    assert!(ids(&list("trashed=true").await).is_empty());
    assert!(ids(&list("folder=Inbox").await).is_empty());
}
