//! Each reader's `unread` column is put back to what their own message
//! rows say.
//!
//! A split used to take read state from the message's shared blob, which
//! one holder's read had set for every holder, so a conversation could
//! carry an unread message and be missing from its reader's Unread list.
//! `maintenance:recompute-unread-column` corrects those rows, on
//! conversations another account holds unless `all=true`; it reports per
//! account how many would turn unread and how many would turn read.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

const ALICE: &str = "alice@x.com";
const BOB: &str = "bob@x.com";
const CAROL: &str = "carol@y.com";

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
    Arc::new(mailrs_fastcore::FastcoreState::new(s))
}

fn deliver(s: &mailrs_fastcore::FastcoreState, user: &str, tid: &str, mid: &str, date: i64) {
    let payload = format!(
        r#"{{"message_id":"{mid}","sender":"{CAROL}","thread_id":"{tid}","internal_date":{date},"subject":"s","flags":0}}"#
    );
    let file = format!("{date}.{mid}.{user}");
    s.mailbox
        .deliver_message(
            &mailrs_mailbox_kevy::MessageArrival {
                thread_id: tid,
                user,
                subject: "s",
                senders_csv: CAROL,
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

fn badge(s: &mailrs_fastcore::FastcoreState, user: &str) -> usize {
    s.mailbox
        .count_flag_in_buckets(user, "unread", &["inbox"])
        .expect("count")
}

async fn call(state: &Arc<mailrs_fastcore::FastcoreState>, query: &str) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!(
                    "/v1/admin/maintenance:recompute-unread-column{query}"
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
async fn columns_are_put_back_per_reader_and_a_second_run_finds_nothing() {
    let s = state();
    // Columns that disagree with the message rows, written the one way the
    // store still can: each half of a read on its own.
    // t1 is held by both; Bob's unread copy lost its column.
    deliver(&s, ALICE, "t1", "m1", 100);
    deliver(&s, BOB, "t1", "m1", 100);
    s.mailbox.mark_seen(BOB, "t1").expect("column");
    // t2 is held by both; Alice's messages are read and her column unread.
    deliver(&s, ALICE, "t2", "m2", 200);
    deliver(&s, BOB, "t2", "m2", 200);
    s.mailbox
        .mark_thread_messages_seen(ALICE, "t2")
        .expect("rows");
    // t3 is Bob's alone and disagrees too.
    deliver(&s, BOB, "t3", "m3", 300);
    s.mailbox.mark_seen(BOB, "t3").expect("column");
    assert_eq!(badge(&s, BOB), 1, "premise: Bob's t1 and t3 are missing");

    let dry = call(&s, "").await;
    assert_eq!(dry["dry_run"], true);
    assert_eq!(dry["walked"], 5, "{dry}");
    assert_eq!(dry["turns_unread"], 1, "{dry}");
    assert_eq!(dry["turns_read"], 1, "{dry}");
    assert_eq!(dry["by_user"][BOB]["turns_unread"], 1, "{dry}");
    assert_eq!(dry["by_user"][BOB]["turns_read"], 0, "{dry}");
    assert_eq!(dry["by_user"][ALICE]["turns_read"], 1, "{dry}");
    assert_eq!(dry["by_user"][BOB]["kept_single_owner"], 1, "{dry}");
    assert_eq!(badge(&s, BOB), 1, "a dry run writes nothing");

    // Batched: Alice's two pairs first.
    let first = call(&s, "?dry_run=false&limit=2").await;
    assert_eq!(first["changed"], 1, "{first}");
    assert_eq!(first["next_skip"], 2);
    assert_eq!(first["done"], false);
    let rest = call(&s, "?dry_run=false&skip=2").await;
    assert_eq!(rest["changed"], 1, "{rest}");
    assert_eq!(badge(&s, BOB), 2, "t1 is back, t3 kept");
    assert_eq!(badge(&s, ALICE), 1, "only t1");

    let again = call(&s, "").await;
    assert_eq!(again["would_change"], 0, "{again}");

    let all = call(&s, "?all=true&dry_run=false").await;
    assert_eq!(all["changed"], 1, "{all}");
    assert_eq!(badge(&s, BOB), 3);
    let all_again = call(&s, "?all=true").await;
    assert_eq!(all_again["would_change"], 0, "{all_again}");
}
