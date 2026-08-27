//! Two properties of the retro-scan, both learned the hard way.
//!
//! **Dry by default.** A new signal meeting a real mailbox for the
//! first time has to say what it would do before it does it — the
//! shadow that ran against production on 2026-08-02 reported 19,779
//! differences, of which 74 were the defect.
//!
//! **Bounded.** An unbounded sweep over this same mailbox took the mail
//! service down for half an hour on 2026-08-26; the store has one lock
//! and a walk that never pauses starves every reader behind it.

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

async fn rescan(state: &Arc<mailrs_fastcore::FastcoreState>, query: &str) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/v1/admin/maintenance:fraud-rescan?{query}"))
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("call");
    assert_eq!(res.status(), StatusCode::OK, "refused: {query}");
    serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .expect("body"),
    )
    .expect("json")
}

#[tokio::test]
async fn it_is_dry_unless_told_otherwise() {
    let tmp = tempfile::tempdir().expect("tmp");
    unsafe { std::env::set_var("MAILRS_MAILDIR", tmp.path()) };
    let state = Arc::new(mailrs_fastcore::FastcoreState::new(store()));

    // No `dry_run` in the query at all.
    let out = rescan(&state, "pause_ms=0").await;
    assert_eq!(out["dry_run"], true, "the default moved mail");
    assert_eq!(out["moved_to_junk"], 0);
}

#[tokio::test]
async fn a_call_stops_at_its_limit_and_says_where_to_resume() {
    let tmp = tempfile::tempdir().expect("tmp");
    unsafe { std::env::set_var("MAILRS_MAILDIR", tmp.path()) };
    let state = Arc::new(mailrs_fastcore::FastcoreState::new(store()));

    let first = rescan(&state, "limit=3&pause_ms=0").await;
    assert_eq!(first["threads_walked"], 3, "the limit was not honoured");
    assert_eq!(first["done"], false);
    assert_eq!(first["next_skip"], 3);

    let last = rescan(&state, "skip=6&limit=3&pause_ms=0").await;
    assert_eq!(last["threads_walked"], 1, "there are seven threads");
    assert_eq!(last["done"], true);
}

/// Adding a destructive action must not have made the safe one stop
/// being the default.
///
/// `action=delete` unlinks maildir files and there is nothing to
/// restore from — the same warning the UI puts in front of a person
/// before one thread. A default that reached it would destroy mail on
/// a URL somebody typed to look.
#[tokio::test]
async fn deleting_is_never_what_a_bare_call_does() {
    let tmp = tempfile::tempdir().expect("tmp");
    unsafe { std::env::set_var("MAILRS_MAILDIR", tmp.path()) };
    let state = Arc::new(mailrs_fastcore::FastcoreState::new(store()));

    let bare = rescan(&state, "pause_ms=0").await;
    assert_eq!(bare["dry_run"], true);
    assert_eq!(bare["deleted"], 0, "a bare call deleted something");

    // Even asked to act, the action is Junk unless `delete` is named.
    let acting = rescan(&state, "dry_run=false&pause_ms=0").await;
    assert_eq!(
        acting["deleted"], 0,
        "acting without naming an action deleted"
    );

    // And a dry run that names `delete` still deletes nothing.
    let dry_delete = rescan(&state, "action=delete&pause_ms=0").await;
    assert_eq!(dry_delete["dry_run"], true);
    assert_eq!(dry_delete["deleted"], 0, "a dry run deleted");
}
