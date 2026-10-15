//! Mail stored before ingest stopped trusting a `Date:` later than the
//! arrival is moved back to its arrival, message and conversation both.
//!
//! The case that prompted it: `base2026@tintgate.com`, delivered
//! 2026-10-12, `Date: Thu, 12 Nov 2026`, so the conversation sat above
//! everything that would arrive for a month. Production also stores
//! rows dated later than their arrival whose file says something
//! earlier and true; those go back to the file's date, not the arrival.
//!
//! One test per binary: the route reads `MAILRS_MAILDIR` from the
//! process environment.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

const USER: &str = "bob@x.com";
const ARRIVED: i64 = 1_791_830_446;
const CLAIMED: i64 = 1_794_508_832;
// what the file of the misdated row says: two seconds before arrival
const WRITTEN: i64 = 1_791_830_444;

#[tokio::test]
async fn a_letter_dated_ahead_of_its_arrival_is_moved_back_once() {
    let tmp = tempfile::tempdir().expect("tmp");
    let cur = tmp.path().join("x.com").join("bob").join("cur");
    std::fs::create_dir_all(&cur).expect("maildir");
    for leaf in ["new", "tmp"] {
        std::fs::create_dir_all(tmp.path().join("x.com").join("bob").join(leaf)).expect("maildir");
    }
    // SAFETY-adjacent: own process, set before the router is built.
    unsafe { std::env::set_var("MAILRS_MAILDIR", tmp.path()) };

    let store =
        Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"));
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(store);
    mailbox.ensure_thread_table();
    mailbox.ensure_admin_indexes();
    mailbox
        .upsert_account(USER, r#"{"address":"bob@x.com","active":true}"#)
        .expect("account");

    // the reported letter; a row stored a day late whose file is right;
    // and an honest one whose date precedes its arrival by a day, as
    // mail delayed in transit does
    let ahead_ref = format!("{ARRIVED}.M1P1Q1.host:2,");
    let misdated_ref = format!("{ARRIVED}.M3P1Q3.host:2,S");
    let honest_ref = format!("{ARRIVED}.M2P1Q2.host:2,");
    for (name, date) in [
        (&ahead_ref, "Thu, 12 Nov 2026 18:40:32 +0000"),
        (&misdated_ref, "Mon, 12 Oct 2026 18:40:44 +0000"),
        (&honest_ref, "Sun, 11 Oct 2026 18:40:46 +0000"),
    ] {
        let raw = format!("From: a@x.com\r\nDate: {date}\r\nSubject: s\r\n\r\nbody\r\n");
        std::fs::write(cur.join(name), raw).expect("file");
    }
    for (mid, blob_ref, date) in [
        ("future@tintgate.com", ahead_ref.as_str(), CLAIMED),
        ("misdated@x.com", misdated_ref.as_str(), ARRIVED + 86_400),
        ("honest@x.com", honest_ref.as_str(), ARRIVED - 86_400),
    ] {
        mailbox
            .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
                category: "inbox",
                is_own: false,
                latest_date: date,
                latest_preview: "body",
                senders_csv: "a@x.com",
                subject: mid,
                thread_id: mid,
                unread: true,
                user: USER,
            })
            .expect("arrival");
        let wire = serde_json::json!({
            "id": 0, "mailbox_id": 0, "uid": 0, "blob_ref": blob_ref,
            "sender": "a@x.com", "recipients": USER, "subject": mid,
            "date": date, "internal_date": date, "size": 32,
            "flags": 0, "message_id": mid, "in_reply_to": "",
            "thread_id": mid, "modseq": 1,
        });
        mailbox
            .upsert_user_message(
                USER,
                mid,
                mid,
                date,
                &serde_json::to_vec(&wire).expect("json"),
                &mailrs_mailbox_kevy::UserMessageFacts {
                    blob_ref,
                    uid: 7,
                    flags: 0,
                    modseq: 1,
                },
            )
            .expect("seed");
    }
    let state = Arc::new(mailrs_fastcore::FastcoreState::new(mailbox));

    let looked = call(&state, false).await;
    assert_eq!(looked["walked"], 3, "{looked}");
    assert_eq!(looked["dated_ahead"], 2, "{looked}");
    assert_eq!(looked["no_file"], 0, "{looked}");
    assert_eq!(looked["repaired"], 0, "a look writes nothing: {looked}");
    assert_eq!(row_date(&state, "future@tintgate.com"), CLAIMED);

    let fixed = call(&state, true).await;
    assert_eq!(fixed["dated_ahead"], 2, "{fixed}");
    assert_eq!(fixed["repaired"], 2, "{fixed}");
    assert_eq!(fixed["rows_lowered"], 2, "{fixed}");
    assert_eq!(fixed["errors"], 0, "{fixed}");

    assert_eq!(message_date(&state, "future@tintgate.com"), ARRIVED);
    assert_eq!(row_date(&state, "future@tintgate.com"), ARRIVED);
    assert_eq!(message_date(&state, "misdated@x.com"), WRITTEN);
    assert_eq!(row_date(&state, "misdated@x.com"), WRITTEN);
    assert_eq!(message_date(&state, "honest@x.com"), ARRIVED - 86_400);
    assert_eq!(row_date(&state, "honest@x.com"), ARRIVED - 86_400);
    let facts = state
        .mailbox
        .user_message_facts(USER, "future@tintgate.com")
        .expect("read")
        .expect("the copy");
    assert_eq!(facts.uid, 7, "the rewrite keeps this user's own facts");
    assert_eq!(facts.blob_ref, ahead_ref);

    let again = call(&state, true).await;
    assert_eq!(again["walked"], 3, "{again}");
    assert_eq!(again["dated_ahead"], 0, "second run still finding: {again}");
}

fn message_date(state: &mailrs_fastcore::FastcoreState, tid: &str) -> i64 {
    let rows = state.mailbox.list_thread_messages(USER, tid).expect("read");
    let wire: mailrs_core_api::method::message::MessageWire =
        serde_json::from_slice(rows.first().expect("the message")).expect("wire");
    wire.internal_date
}

// the membership row is what every list sorts on
fn row_date(state: &mailrs_fastcore::FastcoreState, tid: &str) -> i64 {
    state
        .mailbox
        .get_thread_for_user(USER, tid)
        .expect("read")
        .expect("the row")
        .latest_date
}

async fn call(state: &Arc<mailrs_fastcore::FastcoreState>, repair: bool) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/admin/maintenance:future-dates")
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"repair":{repair}}}"#)))
                .expect("req"),
        )
        .await
        .expect("call");
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json")
}
