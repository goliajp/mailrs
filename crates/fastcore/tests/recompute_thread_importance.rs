//! Each reader's importance verdict is put back on their own row.
//!
//! The verdict was once copied from the shared thread hash onto whichever
//! member's row was written next, so rows carry other members' scores.
//! `maintenance:recompute-thread-importance` corrects them: from the
//! reader's decision log, else a rescore of their own newest inbound
//! message, else unscored. A dry run changes nothing, and a second real run
//! changes nothing either.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

const ALICE: &str = "alice@x.com";
const BOB: &str = "bob@x.com";

fn store() -> mailrs_mailbox_kevy::KevyMailboxStore {
    let s = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"),
    ));
    s.ensure_thread_table();
    s.ensure_admin_indexes();
    for user in [ALICE, BOB] {
        s.upsert_account(user, &format!(r#"{{"address":"{user}","active":true}}"#))
            .expect("account");
    }
    s
}

async fn call(state: &Arc<mailrs_fastcore::FastcoreState>, query: &str) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!(
                    "/v1/admin/maintenance:recompute-thread-importance{query}"
                ))
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

fn maildir(root: &std::path::Path, local: &str) {
    let dir = root.join("x.com").join(local);
    for leaf in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(dir.join(leaf)).expect("maildir");
    }
}

/// Through the spool drain, the way mail reaches a mailbox in production.
fn spool(incoming: &std::path::Path, name: &str, from: &str, to: &[&str], raw: &[u8]) {
    let env = mailrs_core::spool::SpoolEnvelope {
        reverse_path: from.into(),
        forward_paths: to.iter().map(|t| t.to_string()).collect(),
        is_authenticated: false,
        auth_user: None,
        conn_id: 1,
        target_folder: "INBOX".into(),
        received_at: 1,
        fraud_verdict: None,
        sandbox_ns: None,
        schema_version: mailrs_core::spool::SPOOL_SCHEMA_VERSION,
    };
    std::fs::write(
        incoming.join(name),
        mailrs_core::spool::encode_spool_blob(&env, raw),
    )
    .expect("spool");
}

const SHARED: &[u8] = b"From: carol@y.com\r\nTo: alice@x.com, bob@x.com\r\n\
Subject: plans\r\nMessage-ID: <m-1@y.com>\r\nDate: Fri, 14 Aug 2026 01:00:00 +0000\r\n\r\n\
see you friday\r\n";

const BOBS_OWN: &[u8] = b"From: bob@x.com\r\nTo: dave@y.com\r\n\
Subject: mine\r\nMessage-ID: <own-1@x.com>\r\nDate: Fri, 14 Aug 2026 02:00:00 +0000\r\n\r\n\
a note\r\n";

fn verdict(s: &Arc<mailrs_fastcore::FastcoreState>, user: &str, tid: &str) -> (String, f64) {
    let r = s
        .mailbox
        .get_thread_for_user(user, tid)
        .expect("row")
        .expect("present");
    (r.importance_level, r.importance_score)
}

/// One test per binary: the maildir root comes from the process
/// environment.
#[tokio::test]
async fn wrong_verdicts_are_corrected_per_reader_and_the_second_run_changes_nothing() {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = tmp.path().join("maildir");
    maildir(&root, "alice");
    maildir(&root, "bob");
    let incoming = tmp.path().join("spool/incoming/new");
    std::fs::create_dir_all(&incoming).expect("spool");
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_MAILDIR", &root);
        std::env::set_var("MAILRS_LOCAL_DOMAINS", "x.com");
    }

    let state = Arc::new(mailrs_fastcore::FastcoreState::new(store()));
    spool(&incoming, "1.M1P1.a", "carol@y.com", &[ALICE, BOB], SHARED);
    spool(&incoming, "2.M2P1.b", BOB, &[BOB], BOBS_OWN);
    let (drained, _) =
        mailrs_fastcore::spool_drain::drain_once(&incoming, root.to_str().unwrap(), &state);
    assert_eq!(drained, 2, "the drain did not take both");
    let shared = "m-1@y.com";
    let own = "own-1@x.com";
    for (user, tid) in [(ALICE, shared), (BOB, shared), (BOB, own)] {
        assert!(
            state
                .mailbox
                .get_thread_for_user(user, tid)
                .expect("row")
                .is_some(),
            "{user} has no row for {tid}, so this test proves nothing"
        );
    }

    // Delivery scores each inbound reader on their own row; Bob's own send
    // is not scored, and does not pick up anybody else's verdict.
    assert!(
        !verdict(&state, ALICE, shared).0.is_empty(),
        "Alice unscored"
    );
    assert!(!verdict(&state, BOB, shared).0.is_empty(), "Bob unscored");
    assert_eq!(verdict(&state, BOB, own), (String::new(), 0.0));

    // Bob's verdicts predate his decision log.
    std::fs::remove_file(mailrs_threadstate::path(root.join("x.com").join("bob")))
        .expect("Bob's log");

    // Alice's own verdict, in her decision log.
    let mut rec = mailrs_threadstate::Record::new(shared, 1);
    rec.importance_level = Some("critical".into());
    rec.importance_score = Some(0.9);
    mailrs_threadstate::append(root.join("x.com").join("alice"), &rec).expect("log");

    // The rows as the shared field left them: Alice's overwritten, Bob's
    // carrying Alice's score, and Bob's own-only conversation a score that
    // was never his.
    let seed = [
        (ALICE, shared, "low", 0.1),
        (BOB, shared, "critical", 0.9),
        (BOB, own, "important", 0.7),
    ];
    for (user, tid, level, score) in seed {
        state
            .mailbox
            .set_thread_importance(user, tid, level, score)
            .expect("seed");
    }

    let dry = call(&state, "").await;
    assert_eq!(
        dry["dry_run"], true,
        "a call without dry_run is not dry: {dry}"
    );
    assert_eq!(dry["walked"], 3, "{dry}");
    assert_eq!(dry["would_change"], 3, "{dry}");
    assert_eq!(dry["changed"], 0, "{dry}");
    assert_eq!(dry["by_user"][ALICE]["from_log"], 1, "{dry}");
    assert_eq!(dry["by_user"][BOB]["rescored"], 1, "{dry}");
    assert_eq!(dry["by_user"][BOB]["cleared"], 1, "{dry}");
    assert_eq!(
        dry["by_user"][BOB]["another_members_verdict"], 1,
        "Bob's copy of Alice's score is not recognised as hers: {dry}"
    );
    for (user, tid, level, score) in seed {
        assert_eq!(
            verdict(&state, user, tid),
            (level.to_string(), score),
            "the dry run wrote {user}/{tid}"
        );
    }

    let first = call(&state, "?dry_run=false").await;
    assert_eq!(first["changed"], 3, "{first}");
    assert_eq!(first["errors"], 0, "{first}");
    assert_eq!(verdict(&state, ALICE, shared), ("critical".into(), 0.9));
    let bobs = verdict(&state, BOB, shared);
    assert!(!bobs.0.is_empty(), "Bob's inbound copy was left unscored");
    assert_ne!(bobs, ("critical".into(), 0.9), "Bob kept Alice's score");
    assert_eq!(verdict(&state, BOB, own), (String::new(), 0.0));

    let second = call(&state, "?dry_run=false").await;
    assert_eq!(second["walked"], 3, "{second}");
    assert_eq!(second["would_change"], 0, "{second}");
    assert_eq!(second["changed"], 0, "{second}");
    assert_eq!(
        verdict(&state, BOB, shared),
        bobs,
        "the second run moved Bob's"
    );

    // Batched: the same three pairs, one call each.
    let mut skip = 0;
    let mut walked = 0;
    loop {
        let page = call(&state, &format!("?skip={skip}&limit=1")).await;
        walked += page["walked"].as_u64().expect("walked");
        if page["done"] == true {
            break;
        }
        skip = page["next_skip"].as_u64().expect("next_skip");
        assert!(skip <= 3, "the batches do not end: {page}");
    }
    assert_eq!(walked, 3);
}
