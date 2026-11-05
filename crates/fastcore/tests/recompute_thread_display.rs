//! Each reader's subject and preview are put back to their own messages.
//!
//! They were once copied from the shared thread hash, so a member could
//! read the subject and opening line of a message never delivered to them.
//! `maintenance:recompute-thread-display` corrects those rows, on
//! conversations another account holds unless `all=true`, and counts what
//! the rows showed of other people's mail. A dry run changes nothing, and
//! the run after a real one has nothing left to change.

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

struct Mail<'a> {
    mid: &'a str,
    from: &'a str,
    to: &'a str,
    subject: &'a str,
    body: &'a str,
    date: i64,
}

fn deliver(s: &mailrs_fastcore::FastcoreState, user: &str, tid: &str, m: &Mail) {
    let payload = format!(
        r#"{{"message_id":"{}","sender":"{}","recipients":"{}","thread_id":"{tid}","internal_date":{},"subject":"{}","in_reply_to":"x","flags":0}}"#,
        m.mid, m.from, m.to, m.date, m.subject
    );
    let file = format!("{}.{}.{user}", m.date, m.mid);
    s.mailbox
        .deliver_message(
            &mailrs_mailbox_kevy::MessageArrival {
                thread_id: tid,
                user,
                subject: m.subject,
                senders_csv: m.from,
                latest_date: m.date,
                latest_preview: m.body,
                category: "inbox",
                unread: true,
                is_own: false,
            },
            m.mid,
            payload.as_bytes(),
            &mailrs_mailbox_kevy::UserMessageFacts {
                blob_ref: &file,
                uid: 0,
                flags: 0,
                modseq: 1,
            },
        )
        .expect("deliver");
    s.mailbox.index_message_text(m.mid, m.body).expect("body");
}

/// What the copy from the shared hash left on a row before the fix.
fn plant(s: &mailrs_fastcore::FastcoreState, user: &str, tid: &str, subject: &str, preview: &str) {
    s.mailbox
        .store_ref()
        .hset(
            mailrs_mailbox_kevy::keys::thread_user(user, tid).as_bytes(),
            &[
                (b"subject".as_slice(), subject.as_bytes()),
                (b"latest_preview".as_slice(), preview.as_bytes()),
            ],
        )
        .expect("plant");
}

fn shown(s: &mailrs_fastcore::FastcoreState, user: &str, tid: &str) -> (String, String) {
    let r = s
        .mailbox
        .get_thread_for_user(user, tid)
        .expect("row")
        .expect("present");
    (r.subject, r.latest_preview)
}

async fn call(state: &Arc<mailrs_fastcore::FastcoreState>, query: &str) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!(
                    "/v1/admin/maintenance:recompute-thread-display{query}"
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
    let carols = Mail {
        mid: "m1",
        from: CAROL,
        to: "alice@x.com, bob@x.com",
        subject: "Friday agenda",
        body: "Here is the agenda.",
        date: 100,
    };
    let daves = Mail {
        mid: "m2",
        from: DAVE,
        to: "alice@x.com",
        subject: "Acquisition shortlist",
        body: "Confidential: Quillfeather.",
        date: 200,
    };
    deliver(&s, ALICE, "t1", &carols);
    deliver(&s, BOB, "t1", &carols);
    deliver(&s, ALICE, "t1", &daves);
    plant(&s, BOB, "t1", daves.subject, daves.body);
    // A conversation only Bob holds, whose row disagrees with his mail.
    let own = Mail {
        mid: "m3",
        to: BOB,
        subject: "Lunch",
        body: "Noon?",
        date: 300,
        ..carols
    };
    deliver(&s, BOB, "t2", &own);
    plant(&s, BOB, "t2", "Lunch!", "Noon?");

    let dry = call(&s, "").await;
    assert_eq!(dry["dry_run"], true);
    assert_eq!(dry["walked"], 3, "{dry}");
    assert_eq!(dry["would_change"], 1, "{dry}");
    assert_eq!(dry["shows_foreign_subject"], 1, "{dry}");
    assert_eq!(dry["shows_foreign_preview"], 1, "{dry}");
    assert_eq!(dry["findable_by_foreign_words"], 1, "{dry}");
    assert_eq!(dry["missing_some"], 1, "only Bob lacks one: {dry}");
    assert_eq!(dry["by_user"][BOB]["gap_reply_without_them"], 1, "{dry}");
    assert_eq!(dry["by_user"][BOB]["joined_addressed"], 1, "{dry}");
    assert_eq!(dry["by_user"][BOB]["kept_single_owner"], 1, "{dry}");
    assert_eq!(dry["changed"], 0);
    assert_eq!(
        shown(&s, BOB, "t1").0,
        daves.subject,
        "a dry run writes nothing"
    );

    let first = call(&s, "?dry_run=false&limit=1").await;
    assert_eq!(first["changed"], 0, "Alice's row agrees: {first}");
    assert_eq!(first["next_skip"], 1);
    let rest = call(&s, "?dry_run=false&skip=1").await;
    assert_eq!(rest["changed"], 1, "{rest}");
    assert_eq!(
        shown(&s, BOB, "t1"),
        (carols.subject.into(), carols.body.into())
    );
    assert_eq!(shown(&s, ALICE, "t1").0, daves.subject);
    assert_eq!(shown(&s, BOB, "t2").0, "Lunch!", "single-reader row kept");

    let again = call(&s, "").await;
    assert_eq!(again["would_change"], 0, "{again}");
    assert_eq!(again["shows_foreign_subject"], 0, "{again}");
    assert_eq!(again["shows_foreign_preview"], 0, "{again}");

    let all = call(&s, "?all=true&dry_run=false").await;
    assert_eq!(all["changed"], 1, "{all}");
    assert_eq!(shown(&s, BOB, "t2").0, "Lunch");
    let all_again = call(&s, "?all=true").await;
    assert_eq!(all_again["would_change"], 0, "{all_again}");
}
