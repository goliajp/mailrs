//! The sweep that took the mail service down.
//!
//! `POST /v1/admin/backfill-decode-headers` walked every thread in one
//! call: thirty thousand of them on 2026-08-26, each a maildir read and
//! an HTML parse, half an hour of it. The embedded store has one lock,
//! and a walk that reaches for it without pausing starves every reader
//! behind it — the conversation list simply span, and stopping the
//! sweep needed a SIGKILL because the graceful path could not be
//! scheduled.
//!
//! What is asserted here is the shape that prevents it: a call walks at
//! most `limit` threads, says whether it reached the end, and says
//! where to resume. Without the bound, `done` is true on the first call
//! and this test fails on the row it did not stop at.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

const USER: &str = "bob@x.com";

fn store() -> mailrs_mailbox_kevy::KevyMailboxStore {
    let s = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"),
    ));
    s.ensure_thread_table();
    s.ensure_admin_indexes();
    s.upsert_account(USER, r#"{"address":"bob@x.com","active":true}"#)
        .expect("account");
    for i in 0..7 {
        let row = mailrs_mailbox_kevy::ThreadRow {
            account_id: String::new(),
            thread_id: format!("t{i}"),
            subject: format!("subject {i}"),
            senders_csv: "someone@x.com".into(),
            count: 1,
            unread_count: 0,
            latest_date: 1_700_000_000 + i,
            latest_preview: "a line".into(),
            category: "inbox".into(),
            importance_level: "normal".into(),
            importance_score: 0.0,
            requires_action: false,
            pinned: false,
            archived: false,
            has_action: false,
            sent_count: 0,
            snoozed_until: 0,
            starred: false,
        };
        s.upsert_thread(USER, &row).expect("thread");
    }
    s
}

async fn sweep(state: &Arc<mailrs_fastcore::FastcoreState>, query: &str) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/v1/admin/backfill-decode-headers?{query}"))
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("call");
    assert_eq!(res.status(), StatusCode::OK, "sweep refused: {query}");
    serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .expect("body"),
    )
    .expect("json")
}

#[tokio::test]
async fn a_call_stops_at_its_limit_and_says_where_to_resume() {
    let tmp = tempfile::tempdir().expect("tmp");
    unsafe { std::env::set_var("MAILRS_MAILDIR", tmp.path()) };
    let state = Arc::new(mailrs_fastcore::FastcoreState::new(store()));

    // `pause_ms=0` so the test does not spend its life asleep; the
    // bound is what is being asserted, not the nap.
    let first = sweep(&state, "limit=3&pause_ms=0").await;
    assert_eq!(first["threads_walked"], 3, "the limit was not honoured");
    assert_eq!(first["done"], false, "it claimed to have finished at three");
    assert_eq!(first["next_skip"], 3);

    let second = sweep(&state, "skip=3&limit=3&pause_ms=0").await;
    assert_eq!(second["threads_walked"], 3);
    assert_eq!(second["done"], false);
    assert_eq!(second["next_skip"], 6);

    // The last one is short, and that is how the end is known.
    let third = sweep(&state, "skip=6&limit=3&pause_ms=0").await;
    assert_eq!(third["threads_walked"], 1, "there are seven threads");
    assert_eq!(third["done"], true, "it did not notice the end");
}

/// A call with no bound asked for still has one.
///
/// The default is what an operator gets by typing the URL, which is how
/// the outage happened — so the default, not only the explicit form,
/// has to stop.
#[tokio::test]
async fn the_default_is_bounded_too() {
    let tmp = tempfile::tempdir().expect("tmp");
    unsafe { std::env::set_var("MAILRS_MAILDIR", tmp.path()) };
    let state = Arc::new(mailrs_fastcore::FastcoreState::new(store()));
    let all = sweep(&state, "pause_ms=0").await;
    // Seven rows fit inside the default, so this run finishes — what is
    // asserted is that the answer carries the cursor at all.
    assert_eq!(all["done"], true);
    assert_eq!(all["next_skip"], 7);
}
