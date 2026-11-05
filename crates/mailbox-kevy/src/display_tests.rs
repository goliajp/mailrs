//! A conversation row's subject and preview, and what conversation search
//! finds, are each reader's own: read back from the row the list renders
//! and from `search_threads`, after every kind of mutation.
//!
//! Alice and Bob share a conversation. Carol wrote to both; Dave wrote to
//! Alice only, later. Nothing of Dave's message — its subject, its opening
//! line, his name — may reach Bob's row or Bob's search.

use std::sync::Arc;

use kevy_embedded::{Config, Store};

use crate::{KevyMailboxStore, MessageArrival, UserMessageFacts};

const ALICE: &str = "alice@example.test";
const BOB: &str = "bob@example.test";
const CAROL: &str = "carol@outside.example";
const DAVE: &str = "Dave Zimmerman <dave@outside.example>";

pub(crate) fn store() -> KevyMailboxStore {
    let s = KevyMailboxStore::new(Arc::new(Store::open(Config::default()).unwrap()));
    s.ensure_thread_table();
    s.ensure_admin_indexes();
    s
}

pub(crate) struct Mail<'a> {
    pub mid: &'a str,
    pub from: &'a str,
    pub subject: &'a str,
    pub body: &'a str,
    pub date: i64,
}

pub(crate) const CAROLS: Mail = Mail {
    mid: "m1",
    from: CAROL,
    subject: "Friday agenda",
    body: "Here is the agenda for Friday.",
    date: 100,
};

pub(crate) const DAVES: Mail = Mail {
    mid: "m2",
    from: DAVE,
    subject: "Acquisition shortlist",
    body: "Confidential: the shortlist is Quillfeather.",
    date: 200,
};

fn payload(m: &Mail, tid: &str) -> String {
    format!(
        r#"{{"message_id":"{}","sender":"{}","thread_id":"{tid}","internal_date":{},"subject":"{}","flags":0}}"#,
        m.mid, m.from, m.date, m.subject
    )
}

/// Delivery as fastcore's ingest leaves it: the message row, the arrival,
/// then the body for search.
pub(crate) fn deliver(s: &KevyMailboxStore, user: &str, tid: &str, m: &Mail) {
    let arrival = MessageArrival {
        thread_id: tid,
        user,
        subject: m.subject,
        senders_csv: m.from,
        latest_date: m.date,
        latest_preview: m.body,
        category: "inbox",
        unread: true,
        is_own: crate::senders_csv_contains_user(m.from, user),
    };
    let file = format!("{}.{user}", m.mid);
    let facts = UserMessageFacts {
        blob_ref: &file,
        uid: 0,
        flags: 0,
        modseq: 1,
    };
    s.deliver_message(&arrival, m.mid, payload(m, tid).as_bytes(), &facts)
        .unwrap();
    s.index_message_text(m.mid, m.body).unwrap();
}

/// A reply `user` sent, the order fastcore's sent route writes it in.
pub(crate) fn sent_copy(s: &KevyMailboxStore, user: &str, tid: &str, mid: &str, date: i64) {
    let mine = Mail {
        mid,
        from: user,
        subject: "Re: Friday agenda",
        body: "Thanks, see you there.",
        date,
    };
    s.record_message_arrival(&MessageArrival {
        thread_id: tid,
        user,
        subject: mine.subject,
        senders_csv: user,
        latest_date: date,
        latest_preview: mine.body,
        category: "inbox",
        unread: false,
        is_own: true,
    })
    .unwrap();
    let file = format!("{mid}.{user}");
    s.upsert_user_message(
        user,
        tid,
        mid,
        date,
        payload(&mine, tid).as_bytes(),
        &UserMessageFacts {
            blob_ref: &file,
            uid: 0,
            flags: 0,
            modseq: 1,
        },
    )
    .unwrap();
    s.index_message_text(mid, mine.body).unwrap();
    let shared = s.get_thread(tid).unwrap().unwrap();
    s.upsert_thread(user, &shared).unwrap();
}

fn row(s: &KevyMailboxStore, user: &str, tid: &str) -> (String, String) {
    let r = s
        .get_thread_for_user(user, tid)
        .unwrap()
        .unwrap_or_else(|| panic!("{user} has no row for {tid}"));
    (r.subject, r.latest_preview)
}

fn finds(s: &KevyMailboxStore, user: &str, word: &str) -> Vec<String> {
    s.search_threads(user, word, 10)
        .unwrap()
        .into_iter()
        .map(|(t, _)| t)
        .collect()
}

/// Bob's row and Bob's search say nothing of Dave's message.
fn bob_sees_only_his_own(s: &KevyMailboxStore, tid: &str, step: &str) {
    let (subject, preview) = row(s, BOB, tid);
    assert!(
        !subject.contains("Acquisition"),
        "{step}: Bob's subject is {subject:?}"
    );
    assert!(
        !preview.contains("Quillfeather"),
        "{step}: Bob's preview is {preview:?}"
    );
    for word in ["Acquisition", "Quillfeather", "Zimmerman"] {
        assert!(
            finds(s, BOB, word).is_empty(),
            "{step}: Bob finds the conversation by {word:?}"
        );
    }
}

#[test]
fn a_later_arrival_for_one_member_reaches_only_them() {
    let s = store();
    deliver(&s, ALICE, "t1", &CAROLS);
    deliver(&s, BOB, "t1", &CAROLS);
    deliver(&s, ALICE, "t1", &DAVES);
    assert_eq!(row(&s, ALICE, "t1").0, "Acquisition shortlist");
    assert_eq!(finds(&s, ALICE, "Quillfeather"), ["t1"]);
    bob_sees_only_his_own(&s, "t1", "arrival");
    assert_eq!(finds(&s, BOB, "agenda"), ["t1"], "Bob still finds his own");
}

#[test]
fn a_member_who_joins_with_their_own_reply_sees_only_it() {
    let s = store();
    deliver(&s, ALICE, "t1", &DAVES);
    sent_copy(&s, BOB, "t1", "m3", 300);
    bob_sees_only_his_own(&s, "t1", "own reply");
    assert_eq!(row(&s, BOB, "t1").0, "Re: Friday agenda");
}

#[test]
fn a_member_who_joins_with_no_preview_in_hand_does_not_get_anothers() {
    let s = store();
    deliver(&s, ALICE, "t1", &DAVES);
    // the maildir heal re-announces a message with no body in hand
    let quiet = Mail { body: "", ..CAROLS };
    deliver(&s, BOB, "t1", &quiet);
    bob_sees_only_his_own(&s, "t1", "empty preview");
}

#[test]
fn a_write_from_the_shared_hash_keeps_the_readers_own() {
    let s = store();
    deliver(&s, ALICE, "t1", &CAROLS);
    deliver(&s, BOB, "t1", &CAROLS);
    deliver(&s, ALICE, "t1", &DAVES);
    s.set_thread_display_date(BOB, "t1", 150).unwrap();
    bob_sees_only_his_own(&s, "t1", "display date");
    s.move_category(BOB, "t1", "promotion").unwrap();
    bob_sees_only_his_own(&s, "t1", "folder move");
    s.backfill_thread_user(BOB, 0, 10).unwrap();
    bob_sees_only_his_own(&s, "t1", "membership backfill");
}

#[test]
fn an_expunge_shows_what_is_left_of_the_readers_own() {
    let s = store();
    let early = Mail {
        mid: "m0",
        subject: "Room booking",
        body: "Room 4 is booked.",
        date: 50,
        ..CAROLS
    };
    deliver(&s, BOB, "t1", &early);
    deliver(&s, ALICE, "t1", &CAROLS);
    deliver(&s, BOB, "t1", &CAROLS);
    deliver(&s, ALICE, "t1", &DAVES);
    s.forget_message_file(BOB, "m1", &format!("m1.{BOB}"), |_| false)
        .unwrap();
    assert_eq!(
        row(&s, BOB, "t1"),
        ("Room booking".into(), "Room 4 is booked.".into())
    );
    bob_sees_only_his_own(&s, "t1", "expunge");
    assert!(
        finds(&s, BOB, "agenda").is_empty(),
        "the expunged message's words"
    );
}

#[test]
fn a_split_and_a_merge_show_the_readers_own() {
    let s = store();
    let early = Mail {
        mid: "m0",
        subject: "Room booking",
        body: "Room 4 is booked.",
        date: 50,
        ..CAROLS
    };
    deliver(&s, BOB, "t1", &early);
    deliver(&s, ALICE, "t1", &CAROLS);
    deliver(&s, BOB, "t1", &CAROLS);
    deliver(&s, ALICE, "t1", &DAVES);
    let new = s.split_message_to_new_thread(BOB, "m1").unwrap().unwrap();
    assert_eq!(row(&s, BOB, "t1").0, "Room booking");
    assert_eq!(row(&s, BOB, &new).0, "Friday agenda");
    bob_sees_only_his_own(&s, "t1", "split, old");
    bob_sees_only_his_own(&s, &new, "split, new");

    s.merge_thread_into(BOB, &new, "t1").unwrap();
    assert_eq!(row(&s, BOB, "t1").0, "Friday agenda");
    bob_sees_only_his_own(&s, "t1", "merge");
}

#[test]
fn a_reader_finds_their_own_by_subject_sender_and_preview() {
    let s = store();
    deliver(&s, BOB, "t1", &CAROLS);
    for word in ["Friday", "carol", "agenda"] {
        assert_eq!(finds(&s, BOB, word), ["t1"], "{word}");
    }
    assert!(finds(&s, ALICE, "Friday").is_empty(), "Alice holds nothing");
}

#[test]
fn a_conversation_only_the_reader_holds_keeps_its_preview_on_a_recount() {
    let s = store();
    deliver(&s, BOB, "t1", &CAROLS);
    // a line written by older preview code, for Bob's own message
    s.store()
        .hset(
            crate::keys::thread_user(BOB, "t1").as_bytes(),
            &[(
                b"latest_preview".as_slice(),
                b"Here is the agenda".as_slice(),
            )],
        )
        .unwrap();
    let r = s.recount_display(BOB, "t1", false).unwrap();
    assert!(!r.differs, "{r:?}");
    assert_eq!(row(&s, BOB, "t1").1, "Here is the agenda");
}

#[test]
fn a_recount_with_anothers_message_in_it_puts_back_the_readers_own() {
    let s = store();
    deliver(&s, ALICE, "t1", &CAROLS);
    deliver(&s, BOB, "t1", &CAROLS);
    deliver(&s, ALICE, "t1", &DAVES);
    // what the copy from the shared hash used to leave
    s.store()
        .hset(
            crate::keys::thread_user(BOB, "t1").as_bytes(),
            &[
                (b"subject".as_slice(), DAVES.subject.as_bytes()),
                (b"latest_preview".as_slice(), DAVES.body.as_bytes()),
            ],
        )
        .unwrap();
    let dry = s.recount_display(BOB, "t1", true).unwrap();
    assert!(
        dry.differs && dry.foreign_subject && dry.foreign_preview,
        "{dry:?}"
    );
    assert_eq!(
        row(&s, BOB, "t1").0,
        DAVES.subject,
        "a dry run writes nothing"
    );
    assert!(s.recount_display(BOB, "t1", false).unwrap().differs);
    assert_eq!(
        row(&s, BOB, "t1"),
        (CAROLS.subject.into(), CAROLS.body.into())
    );
    assert_eq!(
        s.recount_display(BOB, "t1", true).unwrap(),
        crate::DisplayRecount {
            subject: CAROLS.subject.into(),
            preview: CAROLS.body.into(),
            ..Default::default()
        },
        "converged"
    );
    bob_sees_only_his_own(&s, "t1", "recount");
}
