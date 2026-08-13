//! Marking a message read must reach the maildir file name, not just the index.
//!
//! Step 2 of `.claude/rfcs/20260814-the-maildir-is-the-store.md`. The file
//! name is what an IMAP client reads, so an index-only write means mail read
//! in the web stays bold in Apple Mail forever — measured on production
//! before this landed: 14,704 messages the index called read and the maildir
//! called unseen, and zero the other way.
//!
//! # Why an integration test and not a unit test
//!
//! The path resolves `MAILRS_MAILDIR` from the environment, and the lib
//! test that covers `read_maildir_file` documents that it is *the only test
//! that sets it* — a second one in the same binary would race, because
//! Rust runs a binary's tests on threads sharing one environment. An
//! integration test is its own process, so it owns the variable.
//!
//! It also goes through the router rather than calling the helper, which is
//! the only way to exercise the thing most likely to be got wrong: the
//! order. The file name is written first on purpose. A crash between the two
//! must leave the index *behind* the truth, which a reindex repairs, never
//! ahead of it, which nothing can.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

const USER: &str = "bob@x.com";
const TID: &str = "t-1@x.com";
const MID: &str = "m-1@x.com";

/// The message file for `USER`, whatever flag suffix it now carries and
/// whichever of `new/` or `cur/` it has moved to.
///
/// There is exactly one message in this fixture, so the first entry found
/// is it — but the search has to cover both directories, because marking a
/// message read is precisely what moves it between them.
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

#[tokio::test]
async fn marking_read_writes_the_file_name_and_the_index() {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = tmp.path().to_path_buf();
    let md_dir = root.join("x.com").join("bob");
    for leaf in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(md_dir.join(leaf)).expect("maildir");
    }

    // The message on disk, unread: in `new/`, no suffix.
    let md = mailrs_maildir::Maildir::open(&md_dir);
    let id = md
        .deliver(b"From: a@x.com\r\nSubject: hi\r\n\r\nbody\r\n")
        .expect("deliver");
    let blob_ref = id.0.clone();
    assert!(
        !file_name(&root).contains(":2,"),
        "arrival must start unread"
    );

    // SAFETY-adjacent: own process, set before the router is built and
    // never changed again.
    unsafe { std::env::set_var("MAILRS_MAILDIR", &root) };

    let store =
        Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"));
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(store);
    // Through the real allocator: `upsert_user_message` writes the per-user
    // row but not the uid -> message-id index, and that index is what
    // `get_message_by_uid` reads. Seeding the row alone makes the route
    // answer 404 from its own handler, which is indistinguishable from the
    // path not matching.
    let uid = mailbox.allocate_uid(USER, MID).expect("uid");

    // The index row for it, also unread (`flags: 0`).
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
    let router = mailrs_fastcore::build_router(state.clone());

    let res = router
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/v1/users/{USER}/messages/{uid}/flags"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"flags":1}"#))
                .expect("req"),
        )
        .await
        .expect("call");
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    // The half that did not exist: an IMAP client reads this name.
    let name = file_name(&root);
    assert!(
        name.contains(":2,") && name.contains('S'),
        "the file name still says unread ({name:?}) — an IMAP client would \
         show this message bold after the web marked it read"
    );

    // And the half that did: the index and the thread counter.
    let facts = state
        .mailbox
        .user_message_facts(USER, MID)
        .expect("facts")
        .expect("row");
    assert_eq!(facts.flags & 1, 1, "index bit not set");
    let row = state
        .mailbox
        .get_thread_for_user(USER, TID)
        .expect("thread")
        .expect("row");
    assert_eq!(row.unread_count, 0, "thread counter not cleared");

    // ── and back the other way, on the same message and the same root ──
    //
    // One test, not two. Two `#[tokio::test]`s in one binary each set
    // `MAILRS_MAILDIR` to their own temp dir, and whichever runs second
    // wins — so the first one's route then looks for its file under the
    // other's root, finds nothing, and reports success having renamed
    // nothing. This file's header warns about exactly that race; the
    // warning applies within a binary too, not only between binaries.
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/v1/users/{USER}/messages/{uid}/flags"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"flags":0}"#))
                .expect("req"),
        )
        .await
        .expect("call");
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let name = file_name(&root);
    assert!(
        !name.contains('S'),
        "the file name still carries S ({name:?}) — an IMAP client would \
         show this message read after the web marked it unread"
    );
    let facts = state
        .mailbox
        .user_message_facts(USER, MID)
        .expect("facts")
        .expect("row");
    assert_eq!(facts.flags & 1, 0, "index bit not cleared");
}
