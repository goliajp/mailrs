//! A message filed in a Maildir++ subfolder (`.Sent/`) is read from that
//! subfolder by the single-message route and by the mbox export. Both
//! used to look for `.Sent/<file>` inside the inbox and came back empty.

mod live;

use reqwest::Method;

const KEVY: &str = "mem://webapi-sent-folder-bodies-are-read";
const ALICE: &str = "alice@example.test";
const TID: &str = "t-1@example.test";
const FILE: &str = "1786000000.M1P1.host";
const BODY: &str = "the text of a sent message";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sent_message_body_is_found_in_its_subfolder() {
    let tmp = tempfile::tempdir().expect("tmp");
    let sent = tmp.path().join("example.test/alice/.Sent");
    for leaf in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(sent.join(leaf)).expect("maildir");
    }
    std::fs::write(
        sent.join("cur").join(format!("{FILE}:2,S")),
        format!("From: {ALICE}\r\nTo: bob@partner.example\r\nSubject: hi\r\n\r\n{BODY}\r\n"),
    )
    .expect("message");
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
        std::env::set_var("MAILRS_MAILDIR", tmp.path());
    }
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;
    let alice = live.login(ALICE, "pw").await;
    live.mailbox.ensure_thread_table();
    live.mailbox
        .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
            category: "inbox",
            is_own: true,
            latest_date: 1_786_000_000,
            latest_preview: "hi",
            senders_csv: ALICE,
            subject: "hi",
            thread_id: TID,
            unread: false,
            user: ALICE,
        })
        .expect("arrival");
    let mid = "m-1@example.test";
    let payload = serde_json::json!({
        "uid": 7, "sender": ALICE, "recipients": "bob@partner.example",
        "subject": "hi", "date": 1_786_000_000, "internal_date": 1_786_000_000,
        "size": 80, "flags": 1, "message_id": mid, "in_reply_to": "",
        "thread_id": TID, "modseq": 1,
    });
    live.mailbox
        .upsert_user_message(
            ALICE,
            TID,
            mid,
            1_786_000_000,
            payload.to_string().as_bytes(),
            &mailrs_mailbox_kevy::UserMessageFacts {
                blob_ref: &format!(".Sent/{FILE}"),
                uid: 7,
                flags: 1,
                modseq: 1,
            },
        )
        .expect("message row");
    live.mailbox.register_uid(ALICE, 7, mid).expect("uid");

    let (status, single) = live
        .call(Method::GET, "/api/mail/messages/7", &alice, None)
        .await;
    assert_eq!(status, 200, "{single}");
    let text = single["text_body"].as_str().unwrap_or_default();
    assert!(text.contains(BODY), "single message: {single}");

    let mbox = live
        .http
        .get(format!("{}/api/mail/export", live.base))
        .bearer_auth(&alice)
        .send()
        .await
        .expect("export");
    assert_eq!(mbox.status(), 200);
    let mbox = mbox.text().await.expect("mbox text");
    assert!(mbox.contains(BODY), "mbox export: {mbox}");
}
