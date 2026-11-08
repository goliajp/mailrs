//! Conversations decided before Reply read `invite` and `mutual` are
//! decided again, once, from their messages: an invitation and a sender
//! the reader never writes to leave Reply, a correspondent stays, a first
//! contact keeps the answer its delivery read and stays New, and a second
//! walk finds nothing left to do.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use mailrs_core_sidestate::families::contacts;
use mailrs_mailbox_kevy::section::{MessageFacts, Section};
use tower::ServiceExt;

const USER: &str = "bob@x.com";
const NET: &str = "mem://reply-section-backfill";

fn plain(from: &str, mid: &str) -> String {
    format!(
        "Received: from mx by x.com; Tue, 3 Nov 2026 02:00:00 +0000\r\n\
         Date: Tue, 3 Nov 2026 01:00:00 +0000\r\n\
         From: {from}\r\nTo: {USER}\r\nSubject: hello\r\nMessage-ID: <{mid}>\r\n\
         Content-Type: text/plain\r\n\r\nhello\r\n"
    )
}

fn invitation(from: &str, mid: &str) -> String {
    format!(
        "Received: from mx by x.com; Tue, 3 Nov 2026 02:00:00 +0000\r\n\
         Date: Tue, 3 Nov 2026 01:00:00 +0000\r\n\
         From: {from}\r\nTo: {USER}\r\nSubject: sync\r\nMessage-ID: <{mid}>\r\n\
         MIME-Version: 1.0\r\nContent-Type: text/calendar; method=REQUEST\r\n\r\n\
         BEGIN:VCALENDAR\r\nVERSION:2.0\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\n\
         UID:sync-1\r\nSUMMARY:sync\r\nDTSTAMP:20261101T000000Z\r\n\
         DTSTART:20261103T010000Z\r\nDTEND:20261103T020000Z\r\n\
         ORGANIZER:mailto:{from}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}

/// One test per binary: the route reads `MAILRS_MAILDIR` from the
/// process environment.
#[tokio::test]
async fn rows_from_before_the_later_facts_are_decided_again_once() {
    let tmp = tempfile::tempdir().expect("tmp");
    let md_dir = tmp.path().join("x.com").join("bob");
    for leaf in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(md_dir.join(leaf)).expect("maildir");
    }
    let md = mailrs_maildir::Maildir::open(&md_dir);
    // SAFETY-adjacent: own process, set before the router is built.
    unsafe { std::env::set_var("MAILRS_MAILDIR", tmp.path()) };

    // the network store lives while a connection to it does
    let mut net = kevy_client::Connection::connect(NET).expect("kevy");
    for (who, sent, received) in [("ann@y.test", b"2", b"3"), ("desk@y.test", b"0", b"9")] {
        net.hset(
            contacts::contact_key(USER, who).as_bytes(),
            &[
                (b"sent_count" as &[u8], sent as &[u8]),
                (b"received_count" as &[u8], received as &[u8]),
            ],
        )
        .expect("counts");
    }

    let store =
        Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"));
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(store);
    mailbox.ensure_thread_table();
    mailbox.ensure_admin_indexes();
    mailbox
        .upsert_account(USER, r#"{"address":"bob@x.com","active":true}"#)
        .expect("account");

    let threads = [
        ("ann", "ann@y.test", plain("Ann <ann@y.test>", "ann@y.test")),
        ("desk", "desk@y.test", plain("desk@y.test", "desk@y.test")),
        (
            "sync",
            "ann@y.test",
            invitation("ann@y.test", "sync@y.test"),
        ),
        (
            "eve",
            "eve@far.test",
            plain("Eve <eve@far.test>", "eve@far.test"),
        ),
    ];
    for (tid, from, raw) in &threads {
        let blob_ref = md.deliver(raw.as_bytes()).expect("deliver").0;
        seed_old_row(&mailbox, tid, from, &blob_ref, *tid == "eve");
    }
    for (tid, ..) in &threads[..3] {
        assert_eq!(section(&mailbox, tid), Some(Section::Reply), "{tid}");
    }
    assert_eq!(section(&mailbox, "eve"), Some(Section::New));

    let mut fc = mailrs_fastcore::FastcoreState::new(mailbox);
    fc.net_url = Some(NET.into());
    let state = Arc::new(fc);

    let dry = backfill(&state, true).await;
    assert_eq!(dry["outdated_facts"], 4, "{dry}");
    assert_eq!(dry["refreshed"], 0, "{dry}");
    assert_eq!(dry["moved"]["reply>other"], 2, "{dry}");
    assert_eq!(dry["section_change"]["reply"], -2, "{dry}");
    assert_eq!(dry["section_change"]["other"], 2, "{dry}");
    assert!(dry["section_change"].get("new").is_none(), "{dry}");
    assert_eq!(
        section(&state.mailbox, "desk"),
        Some(Section::Reply),
        "a dry run wrote"
    );

    let real = backfill(&state, false).await;
    assert_eq!(real["refreshed"], 4, "{real}");
    assert_eq!(real["moved"]["reply>other"], 2, "{real}");
    assert_eq!(section(&state.mailbox, "ann"), Some(Section::Reply));
    assert_eq!(section(&state.mailbox, "desk"), Some(Section::Other));
    assert_eq!(section(&state.mailbox, "sync"), Some(Section::Other));
    assert_eq!(section(&state.mailbox, "eve"), Some(Section::New));

    let second = backfill(&state, false).await;
    assert_eq!(
        second["threads_walked"], 4,
        "it must still read them: {second}"
    );
    assert_eq!(second["outdated_facts"], 0, "{second}");
    assert_eq!(second["refreshed"], 0, "{second}");
    assert_eq!(second["already_decided"], 4, "{second}");
    drop(net);
}

/// A conversation as delivery left it before `invite` and `mutual` were
/// recorded: a person wrote to the reader (a first contact, when `first`),
/// at the time delivery records, and nothing more is on the row.
fn seed_old_row(
    mailbox: &mailrs_mailbox_kevy::KevyMailboxStore,
    tid: &str,
    from: &str,
    blob_ref: &str,
    first: bool,
) {
    let mid = format!("{tid}@y.test");
    mailbox
        .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
            category: "inbox",
            is_own: false,
            latest_date: 1_793_667_600,
            latest_preview: "hello",
            senders_csv: from,
            subject: "hello",
            thread_id: tid,
            unread: true,
            user: USER,
        })
        .expect("arrival");
    let wire = serde_json::json!({
        "id": 0, "mailbox_id": 0, "uid": 0, "blob_ref": blob_ref,
        "sender": from, "recipients": USER, "subject": "hello",
        "date": 1_793_667_600i64, "internal_date": 1_793_667_600i64, "size": 32,
        "flags": 0, "message_id": mid, "in_reply_to": "",
        "thread_id": tid, "modseq": 1,
    });
    mailbox
        .upsert_user_message(
            USER,
            tid,
            &mid,
            1_793_667_600,
            &serde_json::to_vec(&wire).expect("json"),
            &mailrs_mailbox_kevy::UserMessageFacts {
                blob_ref,
                uid: 0,
                flags: 0,
                modseq: 1,
            },
        )
        .expect("seed");
    let facts = MessageFacts {
        direct: true,
        from_person: true,
        first_contact: first,
        mutual: true,
        ..MessageFacts::default()
    };
    mailbox
        .record_section_arrival(&mailrs_mailbox_kevy::SectionArrival {
            user: USER,
            thread_id: tid,
            at: 1_793_667_600,
            is_own: false,
            facts,
            from_addr: from,
        })
        .expect("section");
    // the row as it was written before the two facts existed; it was
    // decided Reply then, by the old rule
    let key = mailrs_mailbox_kevy::keys::thread_user(USER, tid);
    mailbox
        .store_ref()
        .hdel(
            key.as_bytes(),
            &[b"sec_in_invite" as &[u8], b"sec_in_mutual"],
        )
        .expect("strip");
}

fn section(mailbox: &mailrs_mailbox_kevy::KevyMailboxStore, tid: &str) -> Option<Section> {
    mailbox.section_of_thread(USER, tid).expect("section")
}

async fn backfill(state: &Arc<mailrs_fastcore::FastcoreState>, dry_run: bool) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!(
                    "/v1/admin/maintenance:backfill-sections?dry_run={dry_run}&pause_ms=0"
                ))
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
