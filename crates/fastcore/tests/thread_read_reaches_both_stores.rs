//! Reading a *conversation* must reach the same two stores reading a
//! *message* does.
//!
//! `read_state_reaches_both_stores.rs` pinned the per-message route. The
//! per-thread one — the verb the conversation list issues, and the one a
//! person actually uses — wrote only the thread counter. `mark_seen` does
//! sink `\Seen` into the shared message blob, but that blob's `flags` has
//! been stripped to zero by design since stage 5 of the per-user message
//! projection, and no read path consults it: `user_message_view` overlays
//! the per-user row on top. So the write landed in the dead half, the per-user
//! row stayed unread, and the file was never renamed.
//!
//! Two consequences, both measured on production 2026-08-14:
//!
//! - mail read in the web stayed bold in every IMAP client, forever;
//! - `read-state-shadow`'s `unread_count_differs` climbed monotonically —
//!   9, then 32, then 68, then 127 — because every conversation opened in
//!   the web added one thread whose counter said 0 and whose files said
//!   otherwise. A level would have been a stock of old damage; a trend is
//!   a live writer.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request};
use tower::ServiceExt;

const USER: &str = "bob@x.com";
const TID: &str = "t-1@x.com";
const MID: &str = "m-1@x.com";

fn file_name(root: &std::path::Path) -> String {
    let box_dir = root.join("x.com").join("bob");
    ["new", "cur"]
        .iter()
        .filter_map(|leaf| std::fs::read_dir(box_dir.join(leaf)).ok())
        .flat_map(|rd| rd.flatten())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .next()
        .unwrap_or_default()
}

/// One test per binary — the route reads `MAILRS_MAILDIR` from the process
/// environment and two would race for it.
#[tokio::test]
async fn marking_a_conversation_read_writes_the_file_names_and_the_rows() {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = tmp.path().to_path_buf();
    let md_dir = root.join("x.com").join("bob");
    for leaf in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(md_dir.join(leaf)).expect("maildir");
    }

    let md = mailrs_maildir::Maildir::open(&md_dir);
    let id = md
        .deliver(b"From: a@x.com\r\nSubject: hi\r\n\r\nbody\r\n")
        .expect("deliver");
    let blob_ref = id.0.clone();

    unsafe { std::env::set_var("MAILRS_MAILDIR", &root) };

    let store =
        Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"));
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(store);
    mailbox.ensure_thread_table();
    mailbox
        .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
            category: "inbox",
            is_own: false,
            latest_date: 1_786_000_000,
            latest_preview: "body",
            senders_csv: "a@x.com",
            subject: "hi",
            thread_id: TID,
            unread: true,
            user: USER,
        })
        .expect("arrival");

    let uid = mailbox.allocate_uid(USER, MID).expect("uid");
    let wire = serde_json::json!({
        "id": 0, "mailbox_id": 0, "uid": uid, "blob_ref": blob_ref,
        "sender": "a@x.com", "recipients": USER, "subject": "hi",
        "date": 1_786_000_000i64, "internal_date": 1_786_000_000i64, "size": 32,
        "flags": 0, "message_id": MID, "in_reply_to": "",
        "thread_id": TID, "modseq": 1,
    });
    let json = serde_json::to_vec(&wire).expect("json");
    mailbox
        .upsert_user_message(
            USER,
            TID,
            MID,
            1_786_000_000,
            &json,
            &mailrs_mailbox_kevy::UserMessageFacts {
                blob_ref: &blob_ref,
                uid,
                flags: 0,
                modseq: 1,
            },
        )
        .expect("seed");

    let state = Arc::new(mailrs_fastcore::FastcoreState::new(mailbox));
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/v1/users/{USER}/threads/{TID}/read"))
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("call");
    assert!(res.status().is_success());

    // The counter, which is all this ever wrote.
    let row = state
        .mailbox
        .get_thread_for_user(USER, TID)
        .expect("thread")
        .expect("row");
    assert_eq!(row.unread_count, 0, "thread counter not cleared");

    // The row a read actually serves from.
    let facts = state
        .mailbox
        .user_message_facts(USER, MID)
        .expect("facts")
        .expect("row");
    assert_eq!(
        facts.flags & 1,
        1,
        "the per-user row still says unread — the thread counter and the \
         message it counts disagree, which is what read-state-shadow \
         reports as unread_count_differs"
    );

    // The name an IMAP client reads.
    let name = file_name(&root);
    assert!(
        name.contains(":2,") && name.contains('S'),
        "the file name still says unread ({name:?}) — this conversation \
         stays bold in Apple Mail after being read in the web"
    );
}
