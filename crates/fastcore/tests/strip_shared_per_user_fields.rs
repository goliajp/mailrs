//! `maintenance:strip-shared-per-user-fields` clears the per-user fields a
//! pre-stage-5 write left on the shared message blob.
//!
//! A dry run changes nothing, a run in batches changes exactly what one
//! full run does, and the run after a real one has nothing left to change.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

const ALICE: &str = "alice@x.com";
const BOB: &str = "bob@x.com";

fn deliver(s: &mailrs_fastcore::FastcoreState, user: &str, tid: &str, mid: &str, date: i64) {
    let payload = format!(
        r#"{{"message_id":"{mid}","sender":"carol@y.com","thread_id":"{tid}","internal_date":{date},"subject":"s","flags":0}}"#
    );
    let file = format!("{date}.{mid}.{user}");
    s.mailbox
        .deliver_message(
            &mailrs_mailbox_kevy::MessageArrival {
                thread_id: tid,
                user,
                subject: "s",
                senders_csv: "carol@y.com",
                latest_date: date,
                latest_preview: "p",
                category: "inbox",
                unread: true,
                is_own: false,
            },
            mid,
            payload.as_bytes(),
            &mailrs_mailbox_kevy::UserMessageFacts {
                blob_ref: &file,
                uid: 0,
                flags: 0,
                modseq: 1,
            },
        )
        .expect("deliver");
}

/// The shape stage 5 stopped writing: one owner's facts on the blob every
/// owner reads.
fn plant_legacy(s: &mailrs_fastcore::FastcoreState, tid: &str, mid: &str, owner: &str) {
    let legacy = format!(
        r#"{{"message_id":"{mid}","thread_id":"{tid}","blob_ref":"{mid}.copy:2,S","uid":7,"flags":1,"modseq":3,"mailbox_id":2,"user_address":"{owner}","subject":"s"}}"#
    );
    s.mailbox
        .store_ref()
        .set(
            mailrs_mailbox_kevy::keys::message_blob(mid).as_bytes(),
            legacy.as_bytes(),
        )
        .expect("plant");
}

/// Four conversations: t1 shared by both readers with one legacy blob,
/// t2 Alice's with two, t3 Bob's already clean, t4 Bob's with one.
fn state() -> Arc<mailrs_fastcore::FastcoreState> {
    let s = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"),
    ));
    s.ensure_thread_table();
    s.ensure_admin_indexes();
    for user in [ALICE, BOB] {
        s.upsert_account(user, &format!(r#"{{"address":"{user}","active":true}}"#))
            .expect("account");
    }
    let s = Arc::new(mailrs_fastcore::FastcoreState::new(s));
    deliver(&s, ALICE, "t1", "m1", 100);
    deliver(&s, BOB, "t1", "m1", 100);
    deliver(&s, ALICE, "t2", "m2", 200);
    deliver(&s, ALICE, "t2", "m3", 300);
    deliver(&s, BOB, "t3", "m4", 400);
    deliver(&s, BOB, "t4", "m5", 500);
    plant_legacy(&s, "t1", "m1", ALICE);
    plant_legacy(&s, "t2", "m2", ALICE);
    plant_legacy(&s, "t2", "m3", ALICE);
    plant_legacy(&s, "t4", "m5", BOB);
    s
}

fn blobs(s: &mailrs_fastcore::FastcoreState) -> Vec<Vec<u8>> {
    ["m1", "m2", "m3", "m4", "m5"]
        .iter()
        .map(|m| s.mailbox.get_message(m).expect("read").expect("present"))
        .collect()
}

async fn call(state: &Arc<mailrs_fastcore::FastcoreState>, query: &str) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!(
                    "/v1/admin/maintenance:strip-shared-per-user-fields{query}"
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

#[tokio::test]
async fn a_dry_run_reports_and_writes_nothing() {
    let s = state();
    let before = blobs(&s);
    let dry = call(&s, "").await;
    assert_eq!(dry["dry_run"], true);
    // t1 is held by both readers and visited once.
    assert_eq!(dry["threads_total"], 4, "{dry}");
    assert_eq!(dry["walked"], 4, "{dry}");
    assert_eq!(dry["messages_seen"], 5, "{dry}");
    assert_eq!(dry["would_change"], 4, "{dry}");
    assert_eq!(dry["rewritten"], 0, "{dry}");
    assert_eq!(dry["done"], true, "{dry}");
    assert_eq!(blobs(&s), before, "a dry run writes nothing");
}

#[tokio::test]
async fn batches_change_what_one_full_run_does_and_a_second_run_finds_nothing() {
    let whole = state();
    let full = call(&whole, "?dry_run=false").await;
    assert_eq!(full["rewritten"], 4, "{full}");

    let batched = state();
    let mut skip = 0u64;
    let mut rewritten = 0u64;
    let mut calls = 0;
    loop {
        let r = call(&batched, &format!("?dry_run=false&limit=1&skip={skip}")).await;
        calls += 1;
        rewritten += r["rewritten"].as_u64().expect("rewritten");
        skip = r["next_skip"].as_u64().expect("next_skip");
        if r["done"] == true {
            break;
        }
    }
    assert_eq!(calls, 4, "one conversation per call");
    assert_eq!(rewritten, 4);
    assert_eq!(
        blobs(&batched),
        blobs(&whole),
        "batches end where one run does"
    );

    let v: serde_json::Value =
        serde_json::from_slice(&whole.mailbox.get_message("m1").expect("read").expect("m1"))
            .expect("json");
    assert_eq!(v["blob_ref"], "");
    assert_eq!(v["user_address"], "");
    assert_eq!(v["subject"], "s", "shared facts are left alone");

    for s in [&whole, &batched] {
        let again = call(s, "?dry_run=false").await;
        assert_eq!(again["walked"], 4, "{again}");
        assert_eq!(again["would_change"], 0, "{again}");
        assert_eq!(again["rewritten"], 0, "{again}");
    }
}
