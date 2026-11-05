//! Stamping a sender verdict on an old message leaves each reader's read
//! state as that reader left it.
//!
//! `backfill-decode-headers` rewrites a message row to add the verdict. The
//! row it wrote back took `flags` from the message's shared blob, which
//! every holder shares and which stood at whatever the last write left: a
//! member's read made another's copy read, and a shared zero made a read
//! copy unread again. The per-user message backfill had the same write.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

const ALICE: &str = "alice@x.com";
const BOB: &str = "bob@x.com";
const CAROL: &str = "carol@y.com";

const RAW: &[u8] = b"From: carol@y.com\r\n\
Authentication-Results: mx.x.com; spf=pass smtp.mailfrom=y.com; dkim=pass header.d=y.com; dmarc=pass header.from=y.com\r\n\
Subject: s\r\n\
Message-ID: <MID>\r\n\
\r\n\
hello\r\n";

fn state(root: &std::path::Path) -> Arc<mailrs_fastcore::FastcoreState> {
    // SAFETY: the only test in this binary, and it sets the variable before
    // anything reads it.
    unsafe { std::env::set_var("MAILRS_MAILDIR", root) };
    let s = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"),
    ));
    s.ensure_thread_table();
    s.ensure_admin_indexes();
    for user in [ALICE, BOB] {
        s.upsert_account(user, &format!(r#"{{"address":"{user}","active":true}}"#))
            .expect("account");
    }
    Arc::new(mailrs_fastcore::FastcoreState::new(s))
}

/// `user`'s copy of `mid` in `tid`, on disk and in the store, read or not.
fn deliver(
    s: &mailrs_fastcore::FastcoreState,
    root: &std::path::Path,
    user: &str,
    tid: &str,
    mid: &str,
    seen: bool,
) {
    let (local, domain) = user.split_once('@').unwrap();
    let new = root.join(domain).join(local).join("new");
    std::fs::create_dir_all(&new).unwrap();
    std::fs::create_dir_all(root.join(domain).join(local).join("cur")).unwrap();
    std::fs::create_dir_all(root.join(domain).join(local).join("tmp")).unwrap();
    let file = format!("{mid}.{local}");
    let raw = String::from_utf8_lossy(RAW).replace("MID", mid);
    std::fs::write(new.join(&file), raw).unwrap();
    let payload = format!(
        r#"{{"id":0,"mailbox_id":0,"uid":1,"blob_ref":"{file}","sender":"{CAROL}","recipients":"{user}","subject":"s","date":100,"internal_date":100,"size":10,"flags":0,"message_id":"{mid}","in_reply_to":"","thread_id":"{tid}","modseq":1}}"#
    );
    s.mailbox
        .deliver_message(
            &mailrs_mailbox_kevy::MessageArrival {
                thread_id: tid,
                user,
                subject: "s",
                senders_csv: CAROL,
                latest_date: 100,
                latest_preview: "hello",
                category: "inbox",
                unread: !seen,
                is_own: false,
            },
            mid,
            payload.as_bytes(),
            &mailrs_mailbox_kevy::UserMessageFacts {
                blob_ref: &file,
                uid: 1,
                flags: u32::from(seen),
                modseq: 1,
            },
        )
        .expect("deliver");
}

/// The shared blob's `flags`, as a write from before per-reader read state
/// could have left it.
fn plant_shared_flags(s: &mailrs_fastcore::FastcoreState, mid: &str, flags: u64) {
    let key = mailrs_mailbox_kevy::keys::message_blob(mid);
    let store = s.mailbox.store_ref();
    let mut wire: serde_json::Value =
        serde_json::from_slice(&store.get(key.as_bytes()).unwrap().unwrap()).unwrap();
    wire["flags"] = flags.into();
    store
        .set(key.as_bytes(), &serde_json::to_vec(&wire).unwrap())
        .unwrap();
}

fn seen(s: &mailrs_fastcore::FastcoreState, user: &str, mid: &str) -> bool {
    s.mailbox
        .user_message_facts(user, mid)
        .unwrap()
        .unwrap()
        .flags
        & 1
        != 0
}

fn stamped(s: &mailrs_fastcore::FastcoreState, mid: &str) -> bool {
    let wire: serde_json::Value =
        serde_json::from_slice(&s.mailbox.get_message(mid).unwrap().unwrap()).unwrap();
    !wire["sender_trust"].as_str().unwrap_or("").is_empty()
}

async fn backfill(state: &Arc<mailrs_fastcore::FastcoreState>, uri: &str) {
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
}

/// (message, Alice read, Bob read, shared blob flags): every pairing of
/// the two holders' states with a shared value that contradicts one of
/// them, so whichever holder a sweep reaches first is contradicted in
/// both directions — a shared read over an unread copy, and a shared zero
/// over a read one.
const CROSSED: [(&str, bool, bool, u64); 4] = [
    ("m1", false, true, 1),
    ("m2", true, false, 0),
    ("m3", true, false, 1),
    ("m4", false, true, 0),
];

fn crossed(root: &std::path::Path) -> Arc<mailrs_fastcore::FastcoreState> {
    let s = state(root);
    for (mid, alice, bob, shared) in CROSSED {
        let tid = format!("t-{mid}");
        deliver(&s, root, ALICE, &tid, mid, alice);
        deliver(&s, root, BOB, &tid, mid, bob);
        plant_shared_flags(&s, mid, shared);
    }
    s
}

fn each_kept_their_own(s: &mailrs_fastcore::FastcoreState, route: &str) {
    for (mid, alice, bob, shared) in CROSSED {
        assert_eq!(
            seen(s, ALICE, mid),
            alice,
            "{route}: Alice's {mid} (shared {shared})"
        );
        assert_eq!(
            seen(s, BOB, mid),
            bob,
            "{route}: Bob's {mid} (shared {shared})"
        );
    }
}

// One test, because both scenarios point `MAILRS_MAILDIR` at their own
// directory and the variable is the process's.
#[tokio::test]
async fn rewriting_a_message_row_keeps_each_readers_own_read_state() {
    let base = std::env::temp_dir().join(format!("mailrs-readstate-{}", std::process::id()));

    let root = base.join("stamp");
    let s = crossed(&root);
    backfill(&s, "/v1/admin/backfill-decode-headers?pause_ms=0").await;
    assert!(CROSSED.iter().all(|c| stamped(&s, c.0)), "premise: stamped");
    each_kept_their_own(&s, "backfill-decode-headers");

    let root = base.join("rows");
    let s = crossed(&root);
    backfill(&s, "/v1/admin/maintenance:backfill-user-messages").await;
    each_kept_their_own(&s, "backfill-user-messages");

    std::fs::remove_dir_all(&base).unwrap();
}
