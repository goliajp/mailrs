//! Mail from before sections existed was decided by a backfill that called
//! nothing a first contact. A walk reads the store's own history instead:
//! the conversation's newest message from someone else is New when it is
//! the first that sender ever sent the reader, the reader had written to
//! them nothing before it, and the contact counts show no mail the store
//! has lost. A sender whose address names a machine leaves Reply for the
//! feed. A second walk finds nothing left to do.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use mailrs_core_sidestate::families::contacts;
use mailrs_mailbox_kevy::section::{MessageFacts, Section};
use tower::ServiceExt;

const USER: &str = "bob@x.com";
const NET: &str = "mem://old-first-contacts";
/// Tue, 3 Nov 2026 01:00:00 +0000.
const T0: i64 = 1_793_667_600;
const DAY: i64 = 86_400;

fn date(at: i64) -> &'static str {
    match (T0 - at) / DAY {
        0 => "Tue, 3 Nov 2026 01:00:00 +0000",
        1 => "Mon, 2 Nov 2026 01:00:00 +0000",
        2 => "Sun, 1 Nov 2026 01:00:00 +0000",
        _ => "Sat, 31 Oct 2026 01:00:00 +0000",
    }
}

fn mail(from: &str, to: &str, cc: &str, mid: &str, at: i64) -> String {
    format!(
        "Date: {}\r\nFrom: {from}\r\nTo: {to}\r\nCc: {cc}\r\nSubject: hello\r\n\
         Message-ID: <{mid}>\r\nContent-Type: text/plain\r\n\r\nhello\r\n",
        date(at)
    )
}

struct Msg {
    tid: &'static str,
    mid: &'static str,
    from: &'static str,
    to: &'static str,
    cc: &'static str,
    at: i64,
}

const fn msg(
    tid: &'static str,
    mid: &'static str,
    from: &'static str,
    to: &'static str,
    cc: &'static str,
    at: i64,
) -> Msg {
    Msg {
        tid,
        mid,
        from,
        to,
        cc,
        at,
    }
}

/// One test per binary: the route reads `MAILRS_MAILDIR` from the
/// process environment.
#[tokio::test]
async fn old_first_contacts_are_new_and_machines_are_feeds() {
    let tmp = tempfile::tempdir().expect("tmp");
    let md_dir = tmp.path().join("x.com").join("bob");
    for leaf in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(md_dir.join(leaf)).expect("maildir");
    }
    let md = mailrs_maildir::Maildir::open(&md_dir);
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

    let tf = "TestFlight <testflight_no_reply@email.apple.com>";
    let msgs = [
        // a stranger's only letter
        msg("pitch", "pitch@far", "Eve <eve@far.test>", USER, "", T0),
        // ann wrote first in another conversation, and bob answered there
        msg("ann1", "ann1@y", "Ann <ann@y.test>", USER, "", T0 - 3 * DAY),
        msg("ann1", "ann1r@x", USER, "ann@y.test", "", T0 - 2 * DAY),
        msg("ann2", "ann2@y", "ann@y.test", USER, "", T0 - DAY),
        // bob had copied carl on a letter before carl first wrote
        msg(
            "tocarl",
            "tocarl@x",
            USER,
            "dave@z.test",
            "carl@z.test",
            T0 - 2 * DAY,
        ),
        msg("carl", "carl@z", "carl@z.test", USER, "", T0),
        // the store has one letter, the counts say there were two
        msg("gone", "gone@w", "gone@w.test", USER, "", T0),
        // a stranger bob has accepted
        msg("acc", "acc@w", "acc@w.test", USER, "", T0),
        // a machine, decided as a person by the old matcher
        msg("tf", "tf@apple", tf, USER, "", T0),
        // a first contact delivery decided
        msg("fresh", "fresh@v", "fresh@v.test", USER, "", T0),
        // a stranger whose stored sender field is not its From: (a bounce
        // address), as some bulk senders' rows are
        msg("card", "card@c", "Card <info@card.test>", USER, "", T0),
    ];
    for m in &msgs {
        let raw = mail(m.from, m.to, m.cc, m.mid, m.at);
        let blob_ref = md.deliver(raw.as_bytes()).expect("deliver").0;
        seed(&mailbox, m, &blob_ref);
    }

    // the network store lives while a connection to it does
    let mut net = kevy_client::Connection::connect(NET).expect("kevy");
    let counts: [(&str, &[u8], &[u8]); 8] = [
        ("eve@far.test", b"0", b"1"),
        ("ann@y.test", b"1", b"2"),
        ("carl@z.test", b"1", b"1"),
        ("gone@w.test", b"0", b"2"),
        ("acc@w.test", b"0", b"1"),
        ("testflight_no_reply@email.apple.com", b"0", b"1"),
        ("fresh@v.test", b"0", b"1"),
        ("info@card.test", b"0", b"1"),
    ];
    for (who, sent, received) in counts {
        net.hset(
            contacts::contact_key(USER, who).as_bytes(),
            &[
                (b"sent_count" as &[u8], sent),
                (b"received_count" as &[u8], received),
            ],
        )
        .expect("counts");
    }
    contacts::accept(&mut net, USER, "acc@w.test", T0 + DAY).expect("accept");

    for tid in ["pitch", "ann2", "carl", "gone", "acc", "card"] {
        assert_eq!(section(&mailbox, tid), Some(Section::Other), "{tid}");
    }
    assert_eq!(section(&mailbox, "tf"), Some(Section::Reply));
    assert_eq!(section(&mailbox, "fresh"), Some(Section::New));

    let mut fc = mailrs_fastcore::FastcoreState::new(mailbox);
    fc.net_url = Some(NET.into());
    let state = Arc::new(fc);

    let dry = backfill(&state, true).await;
    assert_eq!(dry["outdated_facts"], 0, "{dry}");
    assert_eq!(dry["first_contacts_found"], 2, "{dry}");
    assert_eq!(dry["machines_found"], 1, "{dry}");
    assert_eq!(dry["refreshed"], 0, "{dry}");
    assert_eq!(dry["moved"]["other>new"], 2, "{dry}");
    assert_eq!(dry["moved"]["reply>feed"], 1, "{dry}");
    assert_eq!(dry["moved"].as_object().map(|m| m.len()), Some(2), "{dry}");
    assert_eq!(section(&state.mailbox, "pitch"), Some(Section::Other));

    let real = backfill(&state, false).await;
    assert_eq!(real["refreshed"], 3, "{real}");
    assert_eq!(real["moved"], dry["moved"], "{real}");
    assert_eq!(section(&state.mailbox, "pitch"), Some(Section::New));
    assert_eq!(section(&state.mailbox, "card"), Some(Section::New));
    assert_eq!(section(&state.mailbox, "tf"), Some(Section::Feed));
    for tid in ["ann2", "carl", "gone", "acc"] {
        assert_eq!(section(&state.mailbox, tid), Some(Section::Other), "{tid}");
    }
    assert_eq!(section(&state.mailbox, "fresh"), Some(Section::New));

    let second = backfill(&state, false).await;
    assert_eq!(second["first_contacts_found"], 0, "{second}");
    assert_eq!(second["machines_found"], 0, "{second}");
    assert_eq!(second["refreshed"], 0, "{second}");
    assert_eq!(second["changed"], 0, "{second}");
    assert_eq!(second["moved"], serde_json::json!({}), "{second}");
    drop(net);
}

/// A message as the store holds it, and the section row as a backfill from
/// before left it: no first contact, `mutual` recorded, a person unless the
/// row says otherwise.
fn seed(mailbox: &mailrs_mailbox_kevy::KevyMailboxStore, m: &Msg, blob_ref: &str) {
    let own = m.from == USER;
    mailbox
        .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
            category: "inbox",
            is_own: own,
            latest_date: m.at,
            latest_preview: "hello",
            senders_csv: m.from,
            subject: "hello",
            thread_id: m.tid,
            unread: !own,
            user: USER,
        })
        .expect("arrival");
    let wire = serde_json::json!({
        "id": 0, "mailbox_id": 0, "uid": 0, "blob_ref": blob_ref,
        "sender": stored_sender(m), "recipients": m.to, "subject": "hello",
        "date": m.at, "internal_date": m.at, "size": 32,
        "flags": 0, "message_id": m.mid, "in_reply_to": "",
        "thread_id": m.tid, "modseq": 1,
    });
    mailbox
        .upsert_user_message(
            USER,
            m.tid,
            m.mid,
            m.at,
            &serde_json::to_vec(&wire).expect("json"),
            &mailrs_mailbox_kevy::UserMessageFacts {
                blob_ref,
                uid: 0,
                flags: 0,
                modseq: 1,
            },
        )
        .expect("seed");
    let facts = match own {
        true => MessageFacts {
            to_others: true,
            ..MessageFacts::default()
        },
        false => MessageFacts {
            direct: true,
            from_person: true,
            first_contact: m.tid == "fresh",
            mutual: m.tid == "tf",
            ..MessageFacts::default()
        },
    };
    let from = mailrs_core_sidestate::families::correspondent::first_address(m.from);
    mailbox
        .record_section_arrival(&mailrs_mailbox_kevy::SectionArrival {
            user: USER,
            thread_id: m.tid,
            at: m.at,
            is_own: own,
            facts,
            from_addr: &from,
        })
        .expect("section");
}

fn stored_sender(m: &Msg) -> &'static str {
    match m.tid {
        "card" => "rcard-fix@mkrm.card.test",
        _ => m.from,
    }
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
