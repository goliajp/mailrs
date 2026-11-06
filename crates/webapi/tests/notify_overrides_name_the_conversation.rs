//! A muted or followed conversation is listed by its subject, read from the
//! reader's own row, not by a thread id nobody recognises. The old
//! `[id, choice]` pairs stay for the clients that read them.

mod live;

use reqwest::Method;
use serde_json::json;

const KEVY: &str = "mem://webapi-notify-overrides-name-the-conversation";
const ALICE: &str = "alice@example.test";
const BOB: &str = "bob@example.test";
const TID: &str = "t-1@partner.example";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_conversation_choice_carries_its_subject() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;
    live.add_account(BOB, "pw").await;
    let alice = live.login(ALICE, "pw").await;
    let bob = live.login(BOB, "pw").await;
    live.mailbox.ensure_thread_table();
    live.mailbox
        .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
            category: "inbox",
            is_own: false,
            latest_date: 1_786_000_000,
            latest_preview: "notes attached",
            senders_csv: "Friend <friend@partner.example>",
            subject: "Quarterly report",
            thread_id: TID,
            unread: true,
            user: ALICE,
        })
        .expect("arrival");

    for (who, thread) in [(&alice, TID), (&alice, "t-gone"), (&bob, TID)] {
        let (status, _) = live
            .call(
                Method::PUT,
                &format!("/api/notify/threads/{thread}"),
                who,
                Some(json!({ "value": "mute" })),
            )
            .await;
        assert_eq!(status, 204);
    }

    let (status, got) = live
        .call(Method::GET, "/api/notify/overrides", &alice, None)
        .await;
    assert_eq!(status, 200);
    let mut threads = got["threads"].as_array().expect("threads").clone();
    threads.sort_by_key(|t| t[0].as_str().unwrap_or("").to_string());
    assert_eq!(
        threads,
        vec![
            json!(["t-1@partner.example", "mute"]),
            json!(["t-gone", "mute"])
        ]
    );
    let mut details = got["thread_details"].as_array().expect("details").clone();
    details.sort_by_key(|t| t["thread_id"].as_str().unwrap_or("").to_string());
    assert_eq!(
        details,
        vec![
            json!({ "thread_id": TID, "value": "mute", "subject": "Quarterly report" }),
            // a conversation the reader no longer has: unknown, not ""
            json!({ "thread_id": "t-gone", "value": "mute", "subject": null }),
        ]
    );

    // Bob muted the same id but has no row of it: the subject is read from
    // the reader's own rows, so Alice's subject is not his to see
    let (_, got) = live
        .call(Method::GET, "/api/notify/overrides", &bob, None)
        .await;
    assert_eq!(
        got["thread_details"],
        json!([{ "thread_id": TID, "value": "mute", "subject": null }])
    );
}
