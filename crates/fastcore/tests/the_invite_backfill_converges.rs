//! Mail that arrived before anybody looked at a calendar part gets its
//! invitation read, once.
//!
//! `invite_method` is computed at ingest, so the change that started
//! computing it reaches new mail and reaches stored mail never — and
//! every meeting in every mailbox is stored mail, because production
//! ingested invitations for a year without extracting one. The route
//! exists for exactly those, so the assertions are: it finds them, it
//! writes them, and **a second run changes nothing**.
//!
//! That last one is the assertion `periodic-work-must-converge` is
//! about, and the one a repair can otherwise pass forever by counting
//! attempts instead of changes.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

const USER: &str = "bob@x.com";
const OUTLOOK_REQUEST: &[u8] = include_bytes!("../../ical/tests/fixtures/itip/outlook/request.eml");
const PLAIN: &[u8] =
    b"From: a@x.com\r\nSubject: no meeting\r\nMessage-ID: <p1@x.com>\r\n\r\nhi\r\n";

/// One test per binary: the route reads `MAILRS_MAILDIR` from the
/// process environment, and two of them in one binary would race.
#[tokio::test]
async fn an_invitation_already_in_the_mailbox_is_read_once() {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = tmp.path().to_path_buf();
    let md_dir = root.join("x.com").join("bob");
    for leaf in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(md_dir.join(leaf)).expect("maildir");
    }
    let md = mailrs_maildir::Maildir::open(&md_dir);
    let invite_ref = md.deliver(OUTLOOK_REQUEST).expect("deliver").0;
    let plain_ref = md.deliver(PLAIN).expect("deliver").0;

    // SAFETY-adjacent: own process, set before the router is built.
    unsafe { std::env::set_var("MAILRS_MAILDIR", &root) };

    let store =
        Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"));
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(store);
    mailbox.ensure_thread_table();
    mailbox.ensure_admin_indexes();
    mailbox
        .upsert_account(USER, r#"{"address":"bob@x.com","active":true}"#)
        .expect("account");

    let invite_mid = message_id_of(OUTLOOK_REQUEST);
    for (mid, blob_ref, subject) in [
        (invite_mid.as_str(), invite_ref.as_str(), "invite"),
        ("p1@x.com", plain_ref.as_str(), "no meeting"),
    ] {
        mailbox
            .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
                category: "inbox",
                is_own: false,
                latest_date: 1_786_000_000,
                latest_preview: "body",
                senders_csv: "a@x.com",
                subject,
                thread_id: mid,
                unread: true,
                user: USER,
            })
            .expect("arrival");
        // Rows as they were written before anything read a calendar
        // part: no `invite_method` field at all, which is what
        // `#[serde(default)]` on the wire type is for.
        let wire = serde_json::json!({
            "id": 0, "mailbox_id": 0, "uid": 0, "blob_ref": blob_ref,
            "sender": "a@x.com", "recipients": USER, "subject": subject,
            "date": 1_786_000_000i64, "internal_date": 1_786_000_000i64, "size": 32,
            "flags": 0, "message_id": mid, "in_reply_to": "",
            "thread_id": mid, "modseq": 1,
        });
        let json = serde_json::to_vec(&wire).expect("json");
        mailbox
            .upsert_user_message(
                USER,
                mid,
                mid,
                1_786_000_000,
                &json,
                &mailrs_mailbox_kevy::UserMessageFacts {
                    blob_ref,
                    uid: 0,
                    flags: 0,
                    modseq: 1,
                },
            )
            .expect("seed");
    }

    let state = Arc::new(mailrs_fastcore::FastcoreState::new(mailbox));

    let first = backfill(&state).await;
    assert_eq!(
        first["copies_walked"], 2,
        "both messages must be looked at, or a run that changes nothing \
         cannot be told from a run that read nothing ({first})"
    );
    assert_eq!(
        first["changed"], 1,
        "one of the two carries an invitation ({first})"
    );
    assert_eq!(first["not_an_invitation"], 1, "and one does not ({first})");
    assert_eq!(first["by_method"]["REQUEST"], 1, "{first}");

    // The row now says so — which is what the client tests to decide
    // whether to show a card at all.
    let rows = state
        .mailbox
        .list_thread_messages(USER, &invite_mid)
        .expect("read");
    let wire: mailrs_core_api::method::message::MessageWire =
        serde_json::from_slice(rows.first().expect("the message")).expect("wire");
    assert_eq!(wire.invite_method, "REQUEST");

    // And a second run finds nothing left to do. A repair that reports
    // work forever is a repair nobody can tell has finished.
    let second = backfill(&state).await;
    assert_eq!(
        second["copies_walked"], 2,
        "it must still be reading them ({second})"
    );
    assert_eq!(second["changed"], 0, "second run still changing: {second}");
    assert_eq!(second["already_recorded"], 1, "{second}");
}

fn message_id_of(body: &[u8]) -> String {
    String::from_utf8_lossy(&body[..body.len().min(16 * 1024)])
        .lines()
        .find_map(|l| l.strip_prefix("Message-ID:"))
        .expect("the fixture has a Message-ID")
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .to_string()
}

async fn backfill(state: &Arc<mailrs_fastcore::FastcoreState>) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/admin/maintenance:backfill-invites")
                .body(Body::empty())
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
