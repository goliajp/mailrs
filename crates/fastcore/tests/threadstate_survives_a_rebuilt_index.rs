//! A snooze carries a timestamp, so no bit can hold it.
//!
//! Step 5 of `.claude/rfcs/20260814-the-maildir-is-the-store.md`. Read and
//! archived fit in the file name; `snoozed_until`, a classifier verdict and
//! an importance score do not. They go in an append-only log beside the
//! mail, and the row stays the index that serves them.
//!
//! Zero is a value in this log, not an absence — it un-snoozes — which is
//! the `Null vs Zero` distinction `common/coding-style.md` names, and the
//! reason a replay has to be able to tell "put it back" from "nothing was
//! said about it".

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request};
use tower::ServiceExt;

const USER: &str = "bob@x.com";
const WAKE_AT: i64 = 1_800_000_000;

fn store() -> mailrs_mailbox_kevy::KevyMailboxStore {
    let s = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"),
    ));
    s.ensure_thread_table();
    s.ensure_admin_indexes();
    s.upsert_account(USER, r#"{"address":"bob@x.com","active":true}"#)
        .expect("account");
    s
}

async fn send(
    state: &Arc<mailrs_fastcore::FastcoreState>,
    method: Method,
    uri: String,
    body: &str,
) {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .expect("req"),
        )
        .await
        .expect("call");
    assert!(res.status().is_success(), "{}", res.status());
}

fn snoozed_until(state: &Arc<mailrs_fastcore::FastcoreState>, tid: &str) -> i64 {
    state
        .mailbox
        .get_thread_for_user(USER, tid)
        .expect("row")
        .expect("present")
        .snoozed_until
}

/// One test per binary: the maildir root comes from the process
/// environment.
#[tokio::test]
async fn a_snooze_is_replayed_onto_a_rebuilt_index_and_so_is_lifting_it() {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = tmp.path().to_path_buf();
    let md_dir = root.join("x.com").join("bob");
    for leaf in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(md_dir.join(leaf)).expect("maildir");
    }
    mailrs_maildir::Maildir::open(&md_dir)
        .deliver(
            b"From: a@x.com\r\nTo: bob@x.com\r\nSubject: hi\r\n\
              Message-ID: <m-1@x.com>\r\nDate: Fri, 14 Aug 2026 01:00:00 +0000\r\n\r\nbody\r\n",
        )
        .expect("deliver");
    unsafe { std::env::set_var("MAILRS_MAILDIR", &root) };

    let first = Arc::new(mailrs_fastcore::FastcoreState::new(store()));
    mailrs_fastcore::self_heal_once(&first, USER).await;
    let tid = first
        .mailbox
        .all_thread_ids_for_user(USER)
        .expect("threads")
        .into_iter()
        .next()
        .expect("the heal made a thread");
    assert_eq!(snoozed_until(&first, &tid), 0);

    send(
        &first,
        Method::PUT,
        format!("/v1/users/{USER}/threads/{tid}/snooze"),
        &format!(r#"{{"snoozed_until":{WAKE_AT}}}"#),
    )
    .await;
    assert_eq!(snoozed_until(&first, &tid), WAKE_AT, "the row did not take");

    let log = mailrs_threadstate::read(&md_dir).expect("log");
    assert_eq!(
        log.get(&tid).and_then(|r| r.snoozed_until),
        Some(WAKE_AT),
        "the maildir does not carry the snooze, so a rebuild cannot know it"
    );

    // The switch: a new index over the same maildir.
    let second = Arc::new(mailrs_fastcore::FastcoreState::new(store()));
    mailrs_fastcore::self_heal_once(&second, USER).await;
    assert_eq!(
        snoozed_until(&second, &tid),
        WAKE_AT,
        "the rebuilt index shows the thread in the inbox — the snooze was \
         an index column and the index was replaced"
    );

    // And lifting it. Zero is a value: a replay that read it as "nothing
    // said" would put the thread away again on the next rebuild.
    send(
        &second,
        Method::DELETE,
        format!("/v1/users/{USER}/threads/{tid}/unsnooze"),
        "",
    )
    .await;
    let third = Arc::new(mailrs_fastcore::FastcoreState::new(store()));
    mailrs_fastcore::self_heal_once(&third, USER).await;
    assert_eq!(
        snoozed_until(&third, &tid),
        0,
        "the rebuild put the thread back to sleep after it was woken"
    );
}
