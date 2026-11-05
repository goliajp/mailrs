//! Each reader's tab and time are put back to their own messages.
//!
//! They were once copied from the shared thread hash, so one member's move
//! or mail only one member received moved another member's conversation
//! between tabs and re-sorted their list.
//! `maintenance:recompute-thread-standing` corrects those rows, on
//! conversations another account holds unless `all=true`, and counts per
//! account what would move tab and what would re-sort. A dry run changes
//! nothing, and the run after a real one has nothing left to change.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

const ALICE: &str = "alice@x.com";
const BOB: &str = "bob@x.com";

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
    date: i64,
    category: &str,
) {
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
                latest_preview: "",
                category,
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
fn plant(s: &mailrs_fastcore::FastcoreState, user: &str, tid: &str, category: &str, activity: i64) {
    let bucket = mailrs_mailbox_kevy::keys::bucket_of(category).name();
    s.mailbox
        .store_ref()
        .hset(
            mailrs_mailbox_kevy::keys::thread_user(user, tid).as_bytes(),
            &[
                (b"category".as_slice(), category.as_bytes()),
                (b"bucket".as_slice(), bucket.as_bytes()),
                (b"activity".as_slice(), activity.to_string().as_bytes()),
            ],
        )
        .expect("plant");
}

fn shown(s: &mailrs_fastcore::FastcoreState, user: &str, tid: &str) -> (String, i64) {
    let r = s
        .mailbox
        .get_thread_for_user(user, tid)
        .expect("row")
        .expect("present");
    (r.category, r.latest_date)
}

async fn call(state: &Arc<mailrs_fastcore::FastcoreState>, query: &str) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!(
                    "/v1/admin/maintenance:recompute-thread-standing{query}"
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
async fn copied_rows_are_corrected_per_reader_and_a_second_run_finds_nothing() {
    let s = state();
    deliver(&s, ALICE, "t1", "m1", 100, "inbox");
    deliver(&s, BOB, "t1", "m1", 100, "inbox");
    deliver(&s, ALICE, "t1", "m2", 200, "promotion");
    // Alice's tab and time on Bob's row, as the copy left them
    plant(&s, BOB, "t1", "promotion", 200);
    // a conversation only Bob holds, whose row disagrees with his mail
    deliver(&s, BOB, "t2", "m3", 300, "inbox");
    plant(&s, BOB, "t2", "notification", 300);

    let dry = call(&s, "").await;
    assert_eq!(dry["dry_run"], true);
    assert_eq!(dry["walked"], 3, "{dry}");
    assert_eq!(dry["would_change"], 1, "{dry}");
    assert_eq!(dry["tab_moves"], 1, "{dry}");
    assert_eq!(dry["resorts"], 1, "{dry}");
    let bob = &dry["by_user"][BOB];
    assert_eq!(bob["tab_moves_by"]["promotions->inbox"], 1, "{dry}");
    assert_eq!(bob["resorts_earlier"], 1, "{dry}");
    assert_eq!(bob["kept_single_owner"], 1, "{dry}");
    assert_eq!(dry["by_user"][ALICE]["agree"], 1, "{dry}");
    assert_eq!(dry["changed"], 0);
    assert_eq!(
        shown(&s, BOB, "t1"),
        ("promotion".into(), 200),
        "a dry run writes nothing"
    );

    let first = call(&s, "?dry_run=false&limit=1").await;
    assert_eq!(first["changed"], 0, "Alice's row agrees: {first}");
    assert_eq!(first["next_skip"], 1);
    let rest = call(&s, "?dry_run=false&skip=1").await;
    assert_eq!(rest["changed"], 1, "{rest}");
    assert_eq!(shown(&s, BOB, "t1"), ("inbox".into(), 100));
    assert_eq!(shown(&s, ALICE, "t1"), ("promotion".into(), 200));
    assert_eq!(
        shown(&s, BOB, "t2"),
        ("notification".into(), 300),
        "single-reader row kept"
    );

    let again = call(&s, "").await;
    assert_eq!(again["would_change"], 0, "{again}");

    let all = call(&s, "?all=true&dry_run=false").await;
    assert_eq!(all["changed"], 1, "{all}");
    assert_eq!(shown(&s, BOB, "t2"), ("inbox".into(), 300));
    let all_again = call(&s, "?all=true").await;
    assert_eq!(all_again["would_change"], 0, "{all_again}");
}
