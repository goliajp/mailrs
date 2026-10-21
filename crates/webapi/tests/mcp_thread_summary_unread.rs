//! `get_thread_summary` counts a message as unread when it lacks the seen
//! flag. It used to test bit 5, which is `\Recent`, so a read message
//! counted as unread.

mod live;

use mailrs_core_api::method::message::FLAG_SEEN;

const KEVY: &str = "mem://webapi-mcp-thread-summary-unread";
const ALICE: &str = "alice@example.test";
const TID: &str = "t-1@partner.example";

fn message(live: &live::Live, mid: &str, date: i64, flags: u32) {
    let payload = serde_json::json!({
        "uid": 0, "sender": "friend@partner.example", "recipients": ALICE,
        "subject": "hello", "date": date, "internal_date": date, "size": 10,
        "flags": flags, "message_id": mid, "in_reply_to": "", "thread_id": TID,
        "modseq": 1,
    });
    live.mailbox
        .upsert_user_message(
            ALICE,
            TID,
            mid,
            date,
            payload.to_string().as_bytes(),
            &mailrs_mailbox_kevy::UserMessageFacts {
                blob_ref: mid,
                uid: 0,
                flags,
                modseq: 1,
            },
        )
        .expect("message");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_read_message_is_not_counted_as_unread() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;
    let alice = live.login(ALICE, "pw").await;
    live.mailbox.ensure_thread_table();
    live.mailbox
        .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
            category: "inbox",
            is_own: false,
            latest_date: 1_786_000_100,
            latest_preview: "hello",
            senders_csv: "friend@partner.example",
            subject: "hello",
            thread_id: TID,
            unread: true,
            user: ALICE,
        })
        .expect("arrival");
    message(&live, "m-read@partner.example", 1_786_000_000, FLAG_SEEN);
    message(&live, "m-unread@partner.example", 1_786_000_100, 0);

    let mut mcp = live.mcp(&alice).await;
    let summary = mcp
        .tool(
            "get_thread_summary",
            serde_json::json!({ "thread_id": TID }),
        )
        .await
        .expect("get_thread_summary");
    assert_eq!(summary["message_count"], 2, "{summary}");
    assert_eq!(summary["unread_count"], 1, "{summary}");
}
