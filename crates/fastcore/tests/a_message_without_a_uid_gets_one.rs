//! A message with no UID cannot be opened, and 215 on production had none.
//!
//! `uid: 0` is not a cosmetic gap in a row. Viewing the source and
//! downloading any attachment both go through `by-uid`, which answers 404
//! for it, and the web client uses the UID as the timeline's React key —
//! where a repeated zero in one thread is the duplicate-bubble defect of
//! 2026-07-08.
//!
//! Filling them is safe for the one reason that matters about UIDs: no
//! client has ever been given a number for these messages, so there is no
//! promise to break. Changing an existing one would be the opposite, which
//! is why the store's setter refuses to.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

const USER: &str = "bob@x.com";
const TID: &str = "t-1@x.com";
const NO_UID: &str = "imported@x.com";
const HAS_UID: &str = "delivered@x.com";

#[tokio::test]
async fn the_route_fills_the_hole_and_leaves_every_other_uid_alone() {
    let tmp = tempfile::tempdir().expect("tmp");
    // The mailbox has to exist on disk: `uidlist::extend` writes into it
    // and deliberately does not create it, since a directory that is not
    // there means a mailbox that is not there.
    let md_dir = tmp.path().join("x.com").join("bob");
    for leaf in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(md_dir.join(leaf)).expect("maildir");
    }
    unsafe { std::env::set_var("MAILRS_MAILDIR", tmp.path()) };

    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"),
    ));
    mailbox.ensure_thread_table();
    mailbox.ensure_admin_indexes();
    mailbox
        .upsert_account(USER, r#"{"address":"bob@x.com","active":true}"#)
        .expect("account");
    mailbox
        .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
            category: "inbox",
            is_own: false,
            latest_date: 1_786_000_000,
            latest_preview: "body",
            senders_csv: "a@x.com",
            subject: "hi",
            thread_id: TID,
            unread: false,
            user: USER,
        })
        .expect("arrival");

    // An import, and a normally delivered message beside it.
    seed(&mailbox, NO_UID, 0, "imported.host");
    let kept = mailbox.allocate_uid(USER, HAS_UID).expect("uid");
    seed(&mailbox, HAS_UID, kept, "delivered.host");

    let state = Arc::new(mailrs_fastcore::FastcoreState::new(mailbox));
    assert_eq!(
        by_uid(&state, 0).await,
        StatusCode::NOT_FOUND,
        "a message with no uid is reachable, so this test proves nothing"
    );

    let first = run(&state).await;
    assert_eq!(first["uids_allocated"], 1, "{first}");

    let facts = state
        .mailbox
        .user_message_facts(USER, NO_UID)
        .expect("facts")
        .expect("row");
    assert!(facts.uid > 0, "the row still has no uid");
    assert_eq!(
        by_uid(&state, facts.uid).await,
        StatusCode::OK,
        "the message is still unreachable by uid, which is what broke the \
         raw view and every attachment download"
    );

    // The promise is in the maildir too, or a rebuild forgets it.
    let list = mailrs_uidlist::read(tmp.path().join("x.com").join("bob"))
        .expect("read")
        .expect("present");
    assert_eq!(list.uid_of("imported.host"), Some(facts.uid));

    // Untouched, and convergent.
    assert_eq!(
        state
            .mailbox
            .user_message_facts(USER, HAS_UID)
            .expect("facts")
            .expect("row")
            .uid,
        kept,
        "an existing uid was moved — every client holding it is now wrong"
    );
    let second = run(&state).await;
    assert_eq!(second["uids_allocated"], 0, "{second}");
}

fn seed(mailbox: &mailrs_mailbox_kevy::KevyMailboxStore, mid: &str, uid: u32, blob_ref: &str) {
    let wire = serde_json::json!({
        "id": 0, "mailbox_id": 0, "uid": uid, "blob_ref": blob_ref,
        "sender": "a@x.com", "recipients": USER, "subject": "hi",
        "date": 1_786_000_000i64, "internal_date": 1_786_000_000i64, "size": 32,
        "flags": 0, "message_id": mid, "in_reply_to": "",
        "thread_id": TID, "modseq": 1,
    });
    mailbox
        .upsert_user_message(
            USER,
            TID,
            mid,
            1_786_000_000,
            &serde_json::to_vec(&wire).expect("json"),
            &mailrs_mailbox_kevy::UserMessageFacts {
                blob_ref,
                uid,
                flags: 0,
                modseq: 1,
            },
        )
        .expect("seed");
}

async fn run(state: &Arc<mailrs_fastcore::FastcoreState>) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/admin/maintenance:allocate-missing-uids")
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("call");
    assert_eq!(res.status(), StatusCode::OK);
    serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .expect("body"),
    )
    .expect("json")
}

async fn by_uid(state: &Arc<mailrs_fastcore::FastcoreState>, uid: u32) -> StatusCode {
    mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!("/v1/users/{USER}/messages/by-uid/{uid}"))
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("call")
        .status()
}
