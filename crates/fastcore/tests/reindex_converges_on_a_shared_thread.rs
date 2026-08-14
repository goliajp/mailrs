//! A rebuild has to settle, including on a thread two people share.
//!
//! `recount.rs` states the limitation it inherits: the thread hash has no
//! user segment, so "a thread with two local participants can hold one
//! user's numbers or the other's, and the last sweep wins". That is fine
//! for a sweep run once. It is not fine for `maintenance:reindex`, whose
//! whole contract is that a healthy mailbox reports zero — a route that
//! walks every account rewrites that row once per owner, each to a
//! different answer, and reports work forever.
//!
//! Measured on production 2026-08-15 before this was fixed: the reindex
//! dry run reported 72 threads needing a recount, and
//! `maintenance:shadow-counts` reported `diverged_shared: 18` of
//! `scanned: 18` for `devops@golia.jp` — every thread that account has is
//! shared with another local one.
//!
//! So the reindex repairs the **per-user** row, which can be right for
//! everybody, and leaves the shared row to the sweep that owns it.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

const A: &str = "alice@x.com";
const B: &str = "bob@x.com";
const TID: &str = "t-shared@x.com";

fn store() -> mailrs_mailbox_kevy::KevyMailboxStore {
    let s = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"),
    ));
    s.ensure_thread_table();
    s.ensure_admin_indexes();
    for u in [A, B] {
        s.upsert_account(u, &format!(r#"{{"address":"{u}","active":true}}"#))
            .expect("account");
    }
    s
}

/// One message, delivered to both mailboxes — the shape that makes the
/// shared row unable to describe either of them.
fn seed(s: &mailrs_mailbox_kevy::KevyMailboxStore, user: &str, mid: &str, sender: &str) {
    s.record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
        category: "inbox",
        is_own: sender == user,
        latest_date: 100,
        latest_preview: "body",
        senders_csv: sender,
        subject: "shared",
        thread_id: TID,
        unread: sender != user,
        user,
    })
    .expect("arrival");
    let wire = serde_json::json!({
        "id": 0, "mailbox_id": 0, "uid": 0, "blob_ref": format!("{mid}.host"),
        "sender": sender, "recipients": user, "subject": "shared",
        "date": 100, "internal_date": 100, "size": 10,
        "flags": 0, "message_id": mid, "in_reply_to": "",
        "thread_id": TID, "modseq": 1,
    });
    s.upsert_user_message(
        user,
        TID,
        mid,
        100,
        &serde_json::to_vec(&wire).expect("json"),
        &mailrs_mailbox_kevy::UserMessageFacts {
            blob_ref: &format!("{mid}.host"),
            uid: 0,
            flags: 0,
            modseq: 1,
        },
    )
    .expect("seed");
}

async fn reindex(state: &Arc<mailrs_fastcore::FastcoreState>) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/admin/maintenance:reindex")
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

#[tokio::test]
async fn a_thread_two_people_share_does_not_make_the_rebuild_report_forever() {
    let tmp = tempfile::tempdir().expect("tmp");
    unsafe { std::env::set_var("MAILRS_MAILDIR", tmp.path()) };

    let mailbox = store();
    // Alice sent it; Bob received it. Their counts genuinely differ —
    // one sent, one unread — which is the point: no single row can hold
    // both, and a rebuild that writes one will be asked to write the
    // other on the very next account it walks.
    seed(&mailbox, A, "m-1@x.com", A);
    seed(&mailbox, B, "m-1@x.com", A);
    let state = Arc::new(mailrs_fastcore::FastcoreState::new(mailbox));

    let first = reindex(&state).await;
    let second = reindex(&state).await;
    let third = reindex(&state).await;

    assert_eq!(
        (
            second["threads_changed"].as_u64(),
            third["threads_changed"].as_u64()
        ),
        (Some(0), Some(0)),
        "the rebuild is still finding work on a settled mailbox — it is \
         rewriting a shared row once per owner, to a different answer each \
         time\n  1st: {first}\n  2nd: {second}\n  3rd: {third}"
    );

    // And each mailbox's own numbers are right, which is what the read
    // path serves.
    for (user, unread, sent) in [(A, 0, 1), (B, 1, 0)] {
        let row = state
            .mailbox
            .get_thread_for_user(user, TID)
            .expect("row")
            .expect("present");
        assert_eq!(
            (row.unread_count, row.sent_count),
            (unread, sent),
            "{user}'s own row does not describe {user}'s mailbox"
        );
    }
}
