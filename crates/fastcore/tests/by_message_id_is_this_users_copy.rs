//! `by-message-id` answers about **this user's** copy.
//!
//! It read the shared blob and returned it as-is. Since stage 5 of the
//! per-user message projection that blob carries no `blob_ref`, `uid`,
//! `flags` or `user_address` — they are stripped on every write because
//! they depend on who is asking — so the route reported `blob_ref: ""` and
//! `uid: 0` for rows that were entirely correct.
//!
//! That is not a cosmetic difference. It is the instrument that sent the
//! 2026-08-14 blank-body investigation down three wrong paths: asked where
//! a message's file was, it answered "nowhere", and the message's own row
//! said `1786669742.M268216P1Q2.fa91c67f45fe` the whole time.
//!
//! `user_message_view` is the one decision about what a user's copy of a
//! message is, and every other reader goes through it.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

const USER: &str = "bob@x.com";
const OTHER: &str = "eve@x.com";
const TID: &str = "t-1@x.com";
const MID: &str = "m-1@x.com";

#[tokio::test]
async fn it_reports_the_rows_own_file_and_uid() {
    let store =
        Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"));
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(store);
    mailbox.ensure_thread_table();

    let wire = serde_json::json!({
        "id": 0, "mailbox_id": 0, "uid": 7, "blob_ref": "bob-copy.host",
        "sender": "a@x.com", "recipients": USER, "subject": "hi",
        "date": 1_786_000_000i64, "internal_date": 1_786_000_000i64, "size": 32,
        "flags": 1, "message_id": MID, "in_reply_to": "",
        "thread_id": TID, "modseq": 3,
    });
    mailbox
        .upsert_user_message(
            USER,
            TID,
            MID,
            1_786_000_000,
            &serde_json::to_vec(&wire).expect("json"),
            &mailrs_mailbox_kevy::UserMessageFacts {
                blob_ref: "bob-copy.host",
                uid: 7,
                flags: 1,
                modseq: 3,
            },
        )
        .expect("seed");

    let state = Arc::new(mailrs_fastcore::FastcoreState::new(mailbox));

    let got = fetch(&state, USER, MID).await.expect("bob has a copy");
    assert_eq!(
        (got["blob_ref"].as_str(), got["uid"].as_u64()),
        (Some("bob-copy.host"), Some(7)),
        "the shared blob's stripped fields were served instead of the row"
    );
    assert_eq!(got["flags"].as_u64(), Some(1), "flags are per-user too");
    assert_eq!(got["subject"].as_str(), Some("hi"), "shared facts survive");

    // And somebody with no copy is told so, rather than handed a blob with
    // another mailbox's message in it. Same answer `user_message_view`
    // gives the thread listing and the uid fetch.
    assert!(
        fetch(&state, OTHER, MID).await.is_none(),
        "a user with no copy was served one"
    );
}

async fn fetch(
    state: &Arc<mailrs_fastcore::FastcoreState>,
    user: &str,
    mid: &str,
) -> Option<serde_json::Value> {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!("/v1/users/{user}/messages/by-message-id/{mid}"))
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("call");
    if res.status() == StatusCode::NOT_FOUND {
        return None;
    }
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .expect("body");
    Some(serde_json::from_slice(&bytes).expect("json"))
}
