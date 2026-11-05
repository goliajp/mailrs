//! A core-api call is answered while a fraud backfill batch is in the middle
//! of judging a conversation, on a runtime with a single worker.
//!
//! The latch is the network kevy: a listener that takes the sweep's
//! connection, reads its first command and does not answer until the test
//! has its core-api reply. While it waits the walk is parked inside a
//! judgement; run on the runtime's one worker, nothing else could run.
use std::{
    io::Read,
    net::TcpListener,
    path::Path,
    sync::{Arc, mpsc},
    time::Duration,
};

use http_body_util::BodyExt;
use mailrs_fastcore::{FastcoreState, fraud_backfill::run_once};
use mailrs_mailbox_kevy::{KevyMailboxStore, ThreadRow, UserMessageFacts};
use tower::ServiceExt;

const USER: &str = "bob@x.com";

fn add(store: &KevyMailboxStore, root: &Path, tid: &str, raw: &[u8]) {
    let md = mailrs_maildir::Maildir::open(root.join("x.com/bob"));
    let blob = md.deliver(raw).unwrap().0;
    let uid = store.allocate_uid(USER, tid).unwrap();
    let wire = serde_json::json!({
        "id":0,"mailbox_id":0,"uid":uid,"blob_ref":blob,
        "sender":"a@example.org","recipients":USER,
        "subject":"hello","date":1788657272i64,"internal_date":1788657272i64,
        "size":raw.len(),"flags":0,"message_id":tid,"in_reply_to":"",
        "thread_id":tid,"modseq":1
    });
    store
        .upsert_thread(
            USER,
            &ThreadRow {
                account_id: String::new(),
                thread_id: tid.into(),
                subject: "hello".into(),
                senders_csv: "a@example.org".into(),
                count: 1,
                unread_count: 1,
                latest_date: 1788657272,
                latest_preview: String::new(),
                category: "inbox".into(),
                importance_level: "normal".into(),
                importance_score: 0.0,
                requires_action: false,
                pinned: false,
                archived: false,
                quarantined: false,
                has_action: false,
                sent_count: 0,
                snoozed_until: 0,
                trashed_at: 0,
                starred: false,
            },
        )
        .unwrap();
    store
        .upsert_user_message(
            USER,
            tid,
            tid,
            1788657272,
            &serde_json::to_vec(&wire).unwrap(),
            &UserMessageFacts {
                blob_ref: &blob,
                uid,
                flags: 0,
                modseq: 1,
            },
        )
        .unwrap();
}

#[test]
fn a_core_api_call_is_answered_while_a_batch_is_judging() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("maildir");
    for leaf in ["new", "cur", "tmp"] {
        std::fs::create_dir_all(root.join("x.com/bob").join(leaf)).unwrap();
    }
    let journal = tmp.path().join("job");
    let rules = tmp.path().join("active.lua");
    std::fs::write(&rules, mailrs_fraud_lua::DEFAULT_SOURCE).unwrap();
    let kevy = TcpListener::bind("127.0.0.1:0").unwrap();
    // One test in its own integration-test process owns these variables.
    unsafe {
        std::env::set_var("MAILRS_MAILDIR", &root);
        std::env::set_var("MAILRS_FRAUD_RULES_FILE", &rules);
        std::env::set_var(
            "MAILRS_KEVY_URL",
            format!("kevy://{}", kevy.local_addr().unwrap()),
        );
    }
    let store = KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    store.ensure_thread_table();
    store.ensure_admin_indexes();
    store
        .upsert_account(
            USER,
            r#"{"address":"bob@x.com","active":true,"display_name":"Bob"}"#,
        )
        .unwrap();
    let raw = b"From: a@example.org\r\nSubject: hello\r\n\r\nbody\r\n";
    for i in 0..3 {
        add(&store, &root, &format!("t{i}"), raw);
    }
    let state = Arc::new(FastcoreState::new(store));

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let batch = rt.spawn({
        let (state, journal, rules) = (state.clone(), journal.clone(), rules.clone());
        async move { run_once(state, &journal, &rules).await }
    });

    // the walk is inside a judgement once its first command arrives
    let (mut conn, _) = kevy.accept().unwrap();
    let mut first = [0u8; 1];
    conn.read_exact(&mut first).unwrap();

    let (tx, rx) = mpsc::channel();
    let router = mailrs_fastcore::build_router(state.clone());
    rt.spawn(async move {
        let response = router
            .oneshot(
                axum::http::Request::get("/v1/users/bob%40x.com/conversations/unseen-count")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        tx.send((status, body)).unwrap();
    });
    // Bounded only so a regression fails instead of hanging: with the walk
    // on the runtime's one worker this call cannot run at all, however long
    // it is given.
    let answered = rx.recv_timeout(Duration::from_secs(60));
    let still_judging = !batch.is_finished();

    // let the walk go: the closed connection fails its kevy reads
    drop(conn);
    drop(kevy);
    let finished = rt.block_on(batch).unwrap();

    let (status, body) = answered.expect("the core-api call waited on the backfill batch");
    assert_eq!(status, 200);
    let count: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(count["count"].is_i64(), "{count}");
    assert!(
        still_judging,
        "the batch had finished before the call was answered, so this proved nothing"
    );
    finished.unwrap();
}
