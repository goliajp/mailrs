//! Rebuilding the index from what the maildir says — for threads that
//! already exist.
//!
//! Step 6 of `.claude/rfcs/20260814-the-maildir-is-the-store.md`. Steps 3,
//! 4 and 5 put UIDs, keyword bits and thread decisions beside the mail,
//! and the self-heal reads them back — **but only on the branch that
//! creates a thread**, because that is the branch that existed to be
//! extended. On a mailbox whose rows are already there, which is every
//! mailbox on production, none of it is read.
//!
//! So the facts were durable and the rebuild was not. `maintenance:reindex`
//! is the rebuild: it walks every thread a user has and puts tier 1 back
//! onto tier 2, whatever the row currently says.
//!
//! It must be able to report zero — a reconcile whose output cannot come
//! out zero is not a verification (`measure-before-you-cut-over`).

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
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

async fn dry_run(state: &Arc<mailrs_fastcore::FastcoreState>) -> serde_json::Value {
    call(state, "/v1/admin/maintenance:reindex?dry_run=true").await
}

async fn reindex(state: &Arc<mailrs_fastcore::FastcoreState>) -> serde_json::Value {
    call(state, "/v1/admin/maintenance:reindex").await
}

async fn call(state: &Arc<mailrs_fastcore::FastcoreState>, uri: &str) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(uri)
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

/// One test per binary: the maildir root comes from the process
/// environment.
#[tokio::test]
async fn an_existing_row_is_rebuilt_from_the_maildir_and_the_second_run_is_quiet() {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = tmp.path().to_path_buf();
    let md_dir = root.join("x.com").join("bob");
    for leaf in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(md_dir.join(leaf)).expect("maildir");
    }
    let md = mailrs_maildir::Maildir::open(&md_dir);
    let id = md
        .deliver(
            b"From: a@x.com\r\nTo: bob@x.com\r\nSubject: hi\r\n\
              Message-ID: <m-1@x.com>\r\nDate: Fri, 14 Aug 2026 01:00:00 +0000\r\n\r\nbody\r\n",
        )
        .expect("deliver");
    unsafe { std::env::set_var("MAILRS_MAILDIR", &root) };

    // A mailbox whose rows already exist — production's shape.
    let state = Arc::new(mailrs_fastcore::FastcoreState::new(store()));
    mailrs_fastcore::self_heal_once(&state, USER).await;
    let tid = state
        .mailbox
        .all_thread_ids_for_user(USER)
        .expect("threads")
        .into_iter()
        .next()
        .expect("the heal made a thread");

    // Tier 1 now says things the row does not: archived, and asleep until
    // WAKE_AT. Written directly, the way another process — or the same one
    // before its index was replaced — would have left them.
    //
    // `pinned` rather than `archived` for the keyword half, and that is
    // not incidental: `set_snoozed` writes `archived` too — "away is
    // archived, and the time is beside it" — so an archived assertion is
    // satisfied by the snooze below whether or not the bit is read at all.
    // The first version of this test asserted `archived` and passed with
    // the keyword replay deliberately disabled.
    let mut kw = mailrs_keywords::Keywords::new();
    let pinned_bit = kw.intern("pinned").expect("a free bit");
    mailrs_keywords::write(&md_dir, &kw).expect("keywords");
    md.mark_processed_with_keywords(&id, &[mailrs_maildir::Flag::Seen], &[pinned_bit])
        .expect("set the bit");
    let mut rec = mailrs_threadstate::Record::new(&tid, 1);
    rec.snoozed_until = Some(WAKE_AT);
    rec.importance_level = Some("high".into());
    rec.importance_score = Some(0.9);
    mailrs_threadstate::append(&md_dir, &rec).expect("log");

    let row = |s: &Arc<mailrs_fastcore::FastcoreState>| {
        s.mailbox
            .get_thread_for_user(USER, &tid)
            .expect("row")
            .expect("present")
    };
    // The self-heal does not put them back: its replay lives on the branch
    // that creates a thread, and this thread is not new.
    mailrs_fastcore::self_heal_once(&state, USER).await;
    assert!(
        !row(&state).pinned,
        "the self-heal already does this, so this test proves nothing"
    );

    // The dry run reports what the real one will do, and does nothing.
    //
    // Its first version skipped three of the four legs and reported zero
    // for them — a clean bill of health from checks that never ran, which
    // is worse than no dry run at all. So the numbers are compared against
    // the real run below rather than merely being non-zero.
    let dry = dry_run(&state).await;
    assert!(!row(&state).pinned, "the dry run wrote to the index: {dry}");

    let first = reindex(&state).await;
    // The three independent legs are exact. `counts_repaired` is not
    // compared, and that is a property rather than an oversight: the
    // recount asks the message rows and the flag replay is what corrects
    // them, so a dry run measuring against today's rows under-reports the
    // recount that follows a replay it did not perform. A dry run cannot
    // predict the consequences of changes it declined to make.
    for leg in ["from_keywords", "from_flags", "from_threadstate"] {
        assert_eq!(
            dry[leg], first[leg],
            "the dry run's {leg} did not match the real run\n  dry: {dry}\n  run: {first}"
        );
    }
    assert!(
        first["threads_changed"].as_u64().unwrap_or(0) >= 1,
        "the reindex changed nothing: {first}"
    );

    // Each leg asserted where it reports, not inferred through another
    // one: the first version read the flag leg's effect off the unread
    // counter, and forcing that leg to write the wrong answer still passed.
    assert!(
        first["from_flags"].as_u64().unwrap_or(0) >= 1,
        "the `S` on the file name was not put back onto the row: {first}"
    );
    assert!(
        first["from_keywords"].as_u64().unwrap_or(0) >= 1,
        "the keyword bit was not read: {first}"
    );
    assert!(
        first["from_threadstate"].as_u64().unwrap_or(0) >= 1,
        "the decision log was not replayed: {first}"
    );

    let r = row(&state);
    assert!(r.pinned, "the keyword bit was not put back");
    assert_eq!(r.snoozed_until, WAKE_AT, "the snooze was not put back");
    assert_eq!(r.importance_level, "high", "the verdict was not put back");

    // Convergent: having done the work, there is none left. A reconcile
    // that cannot report zero is not a verification.
    let second = reindex(&state).await;
    assert_eq!(
        second["threads_changed"], 0,
        "the second run changed things again: {second}"
    );
    assert!(
        second["threads_walked"].as_u64().unwrap_or(0) >= 1,
        "it reported no work and also walked nothing, which are different \
         things and must not look the same: {second}"
    );

    // There was a third leg here — the counters — and it is gone with
    // the repair machinery (C5c). `unread_count` is no longer a stored
    // number a reindex could recompute; the declared index derives it
    // from the per-user message rows, which the flag replay above is
    // what corrects. Nothing to bend, nothing to recount, nothing to
    // assert.

    assert_eq!(
        reindex(&state).await["threads_changed"],
        0,
        "and it settles again"
    );
}
