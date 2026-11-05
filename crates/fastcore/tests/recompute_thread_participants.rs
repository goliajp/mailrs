//! Each reader's participant list is put back to the senders of their own
//! messages.
//!
//! The list was once copied from the shared thread hash, so a member who
//! joined a conversation later listed people who never wrote to them.
//! `maintenance:recompute-thread-participants` corrects those rows, only on
//! conversations another account holds unless `all=true`. A dry run changes
//! nothing, and the run after a real one has nothing left to change.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

const ALICE: &str = "alice@x.com";
const BOB: &str = "bob@x.com";
const CAROL: &str = "carol@y.com";
const DAVE: &str = "dave@y.com";

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

fn deliver(
    s: &mailrs_fastcore::FastcoreState,
    user: &str,
    tid: &str,
    mid: &str,
    from: &str,
    date: i64,
) {
    let payload = format!(
        r#"{{"message_id":"{mid}","sender":"{from}","thread_id":"{tid}","internal_date":{date},"subject":"s","flags":0}}"#
    );
    let file = format!("{date}.{mid}.{user}");
    s.mailbox
        .deliver_message(
            &mailrs_mailbox_kevy::MessageArrival {
                thread_id: tid,
                user,
                subject: "s",
                senders_csv: from,
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

/// What the copy from the shared hash left on a row before the fix.
fn plant(s: &mailrs_fastcore::FastcoreState, user: &str, tid: &str, csv: &str) {
    s.mailbox
        .store_ref()
        .hset(
            mailrs_mailbox_kevy::keys::thread_user(user, tid).as_bytes(),
            &[(b"senders_csv".as_slice(), csv.as_bytes())],
        )
        .expect("plant");
}

fn listed(s: &mailrs_fastcore::FastcoreState, user: &str, tid: &str) -> String {
    s.mailbox
        .get_thread_for_user(user, tid)
        .expect("row")
        .expect("present")
        .senders_csv
}

async fn call(state: &Arc<mailrs_fastcore::FastcoreState>, query: &str) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!(
                    "/v1/admin/maintenance:recompute-thread-participants{query}"
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
async fn copied_lists_are_corrected_per_reader_and_a_second_run_finds_nothing() {
    let s = state();
    // Alice holds Carol's and Dave's messages; Bob joined with Dave's only.
    deliver(&s, ALICE, "t1", "m1", CAROL, 100);
    deliver(&s, ALICE, "t1", "m2", DAVE, 200);
    deliver(&s, BOB, "t1", "m2", DAVE, 200);
    plant(&s, BOB, "t1", &format!("{CAROL},{DAVE}"));
    // A conversation only Bob holds, whose row disagrees with his mail.
    deliver(&s, BOB, "t2", "m3", CAROL, 300);
    plant(&s, BOB, "t2", &format!("{CAROL},{BOB}"));

    let dry = call(&s, "").await;
    assert_eq!(dry["dry_run"], true);
    assert_eq!(dry["walked"], 3, "{dry}");
    assert_eq!(dry["would_change"], 1, "{dry}");
    assert_eq!(dry["names_a_stranger"], 1, "{dry}");
    assert_eq!(dry["changed"], 0, "{dry}");
    assert_eq!(dry["by_user"][BOB]["kept_single_owner"], 1, "{dry}");
    assert_eq!(
        listed(&s, BOB, "t1"),
        format!("{CAROL},{DAVE}"),
        "a dry run writes nothing"
    );

    // Batched: the first pair is Alice's, which agrees.
    let first = call(&s, "?dry_run=false&limit=1").await;
    assert_eq!(
        (first["walked"].clone(), first["changed"].clone()),
        (1.into(), 0.into())
    );
    assert_eq!(first["next_skip"], 1);
    assert_eq!(first["done"], false);
    let rest = call(&s, "?dry_run=false&skip=1").await;
    assert_eq!(rest["changed"], 1, "{rest}");
    assert_eq!(listed(&s, BOB, "t1"), DAVE);
    assert_eq!(listed(&s, ALICE, "t1"), format!("{CAROL},{DAVE}"));
    assert_eq!(
        listed(&s, BOB, "t2"),
        format!("{CAROL},{BOB}"),
        "single-reader row kept"
    );

    let again = call(&s, "").await;
    assert_eq!(again["would_change"], 0, "{again}");

    // `all=true` reaches the conversation only Bob holds, and converges too.
    let all = call(&s, "?all=true&dry_run=false").await;
    assert_eq!(all["changed"], 1, "{all}");
    assert_eq!(listed(&s, BOB, "t2"), CAROL);
    let all_again = call(&s, "?all=true").await;
    assert_eq!(all_again["would_change"], 0, "{all_again}");
}
