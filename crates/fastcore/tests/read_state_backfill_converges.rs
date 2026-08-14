//! The read-state backfill has to converge the shadow it was written for.
//!
//! Measured on production 2026-08-14, after the 215 repaired `blob_ref`s made
//! these rows visible to the shadow at all:
//!
//! ```text
//! read-state-shadow    seen_only_on_disk: 215
//! read-state-backfill  index_marked_seen:  15   (dry run)
//! ```
//!
//! Two hundred of the two hundred and fifteen were invisible to the repair
//! that exists to fix them, and no number of runs would have moved them —
//! `measure-before-you-cut-over`'s "a verification metric that cannot come
//! out zero", one step upstream: a *repair* whose own shadow cannot reach
//! zero.
//!
//! The cause is one line. A thread whose counter says read while no message
//! does is treated as read for every message in it — correct, and the reason
//! the third case in `backfill_read_state.rs` exists. But that verdict was
//! then compared against the file, so a message the **disk** already calls
//! read matched it, counted as agreement, and left the index row's own bit
//! clear forever.
//!
//! Its own module docstring names the invariant this breaks: "a second run
//! must say `changed: 0`". It did say that. It also said it the first time.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use mailrs_maildir::Flag;
use tower::ServiceExt;

const USER: &str = "bob@x.com";
const TID: &str = "t-1@x.com";
const MID: &str = "m-1@x.com";

/// One test per binary: the route reads `MAILRS_MAILDIR` from the process
/// environment, and two `#[tokio::test]`s in one binary would race for it —
/// the same warning `read_state_reaches_both_stores.rs` carries.
#[tokio::test]
async fn a_message_the_disk_calls_read_gets_its_index_bit_back() {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = tmp.path().to_path_buf();
    let md_dir = root.join("x.com").join("bob");
    for leaf in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(md_dir.join(leaf)).expect("maildir");
    }

    // The file already carries `S` — read in an IMAP client, or repaired by
    // an earlier pass of this very route.
    let md = mailrs_maildir::Maildir::open(&md_dir);
    let id = md
        .deliver(b"From: a@x.com\r\nSubject: hi\r\n\r\nbody\r\n")
        .expect("deliver");
    md.mark_processed(&id, &[Flag::Seen]).expect("mark seen");
    let blob_ref = id.0.clone();

    // SAFETY-adjacent: own process, set before the router is built.
    unsafe { std::env::set_var("MAILRS_MAILDIR", &root) };

    let store =
        Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"));
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(store);
    mailbox.ensure_thread_table();
    mailbox.ensure_admin_indexes();
    mailbox
        .upsert_account(USER, r#"{"address":"bob@x.com","active":true}"#)
        .expect("account");

    // The thread counter says read...
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

    // ...while the message row's own bit is clear. This is the shape the
    // shadow reports as `seen_only_on_disk`.
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

    let first = backfill(&state).await;
    assert_eq!(
        first["index_marked_seen"], 1,
        "the disk says read and the index does not — that is the one \
         direction this route calls steady-state ({first})"
    );

    let facts = state
        .mailbox
        .user_message_facts(USER, MID)
        .expect("facts")
        .expect("row");
    assert_eq!(
        facts.flags & 1,
        1,
        "the index row is still unread, so the shadow still counts this \
         message as seen_only_on_disk and a second run repairs nothing"
    );

    // Convergent, not merely idempotent: having done the work, there is
    // none left.
    let second = backfill(&state).await;
    assert_eq!(
        second["changed"], 0,
        "second run still changing things: {second}"
    );
}

async fn backfill(state: &Arc<mailrs_fastcore::FastcoreState>) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/admin/maintenance:read-state-backfill")
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("call");
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json")
}
