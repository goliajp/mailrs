use std::sync::Arc;

use kevy_embedded::{Config, Store};
use mailrs_mailbox::section::{MessageFacts, Section};

use super::SectionArrival;
use crate::{ArchiveScope, KevyMailboxStore, MessageArrival};

const ME: &str = "me@x.test";

fn store() -> KevyMailboxStore {
    let s = KevyMailboxStore::new(Arc::new(
        Store::open(Config::default()).expect("open in-memory kevy"),
    ));
    s.ensure_thread_table();
    s
}

const PERSON: MessageFacts = MessageFacts {
    direct: true,
    from_person: true,
    first_contact: false,
    to_others: false,
};

/// A message arriving the way the drain records it: the row, then its section.
fn arrive(st: &KevyMailboxStore, tid: &str, at: i64, own: bool, facts: MessageFacts, from: &str) {
    st.record_message_arrival(&MessageArrival {
        thread_id: tid,
        user: ME,
        subject: "s",
        senders_csv: from,
        latest_date: at,
        latest_preview: "p",
        category: "inbox",
        unread: !own,
        is_own: own,
    })
    .unwrap();
    st.record_section_arrival(&SectionArrival {
        user: ME,
        thread_id: tid,
        at,
        is_own: own,
        facts,
        from_addr: from,
    })
    .unwrap();
}

fn mine(to_others: bool) -> MessageFacts {
    MessageFacts {
        to_others,
        ..MessageFacts::default()
    }
}

fn section(st: &KevyMailboxStore, tid: &str) -> Option<Section> {
    st.section_of_thread(ME, tid).unwrap()
}

fn arrival<'a>(at: i64, own: bool, facts: MessageFacts, from: &'a str) -> SectionArrival<'a> {
    SectionArrival {
        user: ME,
        thread_id: "t",
        at,
        is_own: own,
        facts,
        from_addr: from,
    }
}

/// The user's reply goes (an IMAP client deleted it): the conversation is
/// theirs to answer again, which a fold that only moves forward cannot see.
#[test]
fn a_section_is_decided_again_from_what_is_left() {
    let st = store();
    arrive(&st, "t", 10, false, PERSON, "ann@x.test");
    arrive(&st, "t", 20, true, mine(true), ME);
    assert_eq!(section(&st, "t"), Some(Section::Awaiting));
    st.refold_section(ME, "t", &[arrival(10, false, PERSON, "ann@x.test")])
        .unwrap();
    assert_eq!(section(&st, "t"), Some(Section::Reply));
}

/// A first contact that is still the newest inbound message keeps the
/// answer its delivery read, though a refold reads it as not one.
#[test]
fn a_refold_keeps_the_first_contact_its_delivery_read() {
    let st = store();
    let stranger = MessageFacts {
        first_contact: true,
        ..PERSON
    };
    arrive(&st, "t", 10, false, stranger, "eve@far.test");
    arrive(&st, "t", 20, true, mine(true), ME);
    assert_eq!(section(&st, "t"), Some(Section::Awaiting));
    st.refold_section(ME, "t", &[arrival(10, false, PERSON, "eve@far.test")])
        .unwrap();
    assert_eq!(section(&st, "t"), Some(Section::New));
}

#[test]
fn a_reply_moves_a_conversation_out_of_waiting_at_once() {
    let st = store();
    arrive(&st, "t", 10, false, PERSON, "ann@y.test");
    assert_eq!(section(&st, "t"), Some(Section::Reply));
    arrive(&st, "t", 20, true, mine(true), ME);
    assert_eq!(section(&st, "t"), Some(Section::Awaiting));
    // a ticket robot's receipt is not an answer
    arrive(
        &st,
        "t",
        25,
        false,
        MessageFacts::default(),
        "robot@desk.test",
    );
    assert_eq!(section(&st, "t"), Some(Section::Awaiting));
    arrive(&st, "t", 30, false, PERSON, "ann@y.test");
    assert_eq!(section(&st, "t"), Some(Section::Reply));
}

#[test]
fn an_older_message_arriving_late_changes_nothing() {
    let st = store();
    arrive(&st, "t", 20, true, mine(true), ME);
    arrive(&st, "t", 10, false, PERSON, "ann@y.test");
    assert_eq!(section(&st, "t"), Some(Section::Awaiting));
}

#[test]
fn a_first_contact_is_new_until_accepted() {
    let st = store();
    let stranger = MessageFacts {
        first_contact: true,
        ..PERSON
    };
    arrive(&st, "a", 10, false, stranger, "eve@far.test");
    arrive(&st, "b", 11, false, stranger, "zed@far.test");
    assert_eq!(section(&st, "a"), Some(Section::New));
    let moved = st
        .accept_senders(ME, &["eve@far.test".to_string()])
        .unwrap();
    assert_eq!(moved, 1);
    assert_eq!(section(&st, "a"), Some(Section::Reply));
    assert_eq!(section(&st, "b"), Some(Section::New));
}

#[test]
fn a_section_lists_and_counts_its_conversations() {
    let st = store();
    arrive(&st, "r1", 10, false, PERSON, "ann@y.test");
    arrive(&st, "r2", 20, false, PERSON, "bob@y.test");
    arrive(&st, "w", 30, true, mine(true), ME);
    let reply = st
        .list_thread_ids_by_section_via_table(ME, Section::Reply, ArchiveScope::Live, 10)
        .unwrap();
    assert_eq!(reply, vec!["r2", "r1"]);
    let n = st
        .count_thread_ids_by_section_via_table(ME, Section::Reply, ArchiveScope::Live)
        .unwrap();
    assert_eq!(n, 2);
    let older = st
        .list_thread_ids_by_section_before_via_table(ME, Section::Reply, ArchiveScope::Live, 19, 10)
        .unwrap();
    assert_eq!(older, vec!["r1"]);
}

#[test]
fn a_follow_up_is_due_once_its_time_has_passed() {
    let st = store();
    arrive(&st, "old", 100, true, mine(true), ME);
    arrive(&st, "recent", 900, true, mine(true), ME);
    arrive(&st, "note", 950, true, mine(false), ME);
    let all = st.list_awaiting_via_index(ME, None).unwrap();
    assert_eq!(
        all,
        vec![("recent".to_string(), 900), ("old".to_string(), 100)]
    );
    let due = st.list_awaiting_via_index(ME, Some(500)).unwrap();
    assert_eq!(due, vec![("old".to_string(), 100)]);
    arrive(&st, "old", 1000, false, PERSON, "ann@y.test");
    assert!(
        st.list_awaiting_via_index(ME, Some(500))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn junk_has_no_section_and_leaving_it_restores_one() {
    let st = store();
    arrive(&st, "t", 10, false, PERSON, "ann@y.test");
    st.set_bucket(ME, "t", crate::keys::Bucket::Junk).unwrap();
    assert_eq!(section(&st, "t"), None);
    st.set_bucket(ME, "t", crate::keys::Bucket::Inbox).unwrap();
    assert_eq!(section(&st, "t"), Some(Section::Reply));
}

#[test]
fn a_row_with_nothing_recorded_stays_undecided() {
    let st = store();
    st.record_message_arrival(&MessageArrival {
        thread_id: "old",
        user: ME,
        subject: "s",
        senders_csv: "ann@y.test",
        latest_date: 5,
        latest_preview: "p",
        category: "inbox",
        unread: true,
        is_own: false,
    })
    .unwrap();
    st.set_bucket(ME, "old", crate::keys::Bucket::Promotions)
        .unwrap();
    assert_eq!(section(&st, "old"), None);
}

/// The row and `Latest::fold` keep the same rule: the same messages, in
/// several orders, give the stored section the fold gives.
#[test]
fn the_row_follows_the_same_rule_as_the_fold() {
    use mailrs_mailbox::section::{Latest, section_of};
    let msgs: [(i64, bool, MessageFacts, &str); 4] = [
        (10, false, PERSON, "ann@y.test"),
        (20, true, mine(true), ME),
        (25, false, MessageFacts::default(), "robot@desk.test"),
        (30, false, PERSON, "ann@y.test"),
    ];
    for order in [[0, 1, 2, 3], [3, 2, 1, 0], [1, 3, 0, 2]] {
        let st = store();
        let mut latest = Latest::default();
        for i in order {
            let (at, own, facts, from) = msgs[i];
            arrive(&st, "t", at, own, facts, from);
            latest.fold(at, own, facts);
        }
        assert_eq!(section(&st, "t"), Some(section_of(&latest)), "{order:?}");
    }
}

/// Re-emitting the thread row afterwards, as `deliver_message` does, keeps
/// the section: the row is the reader's, and the section is on it.
#[test]
fn rewriting_the_thread_row_keeps_the_section() {
    let st = store();
    arrive(&st, "t", 10, false, PERSON, "ann@y.test");
    let row = st.get_thread("t").unwrap().unwrap();
    st.upsert_thread(ME, &row).unwrap();
    assert_eq!(section(&st, "t"), Some(Section::Reply));
    assert_eq!(
        st.section_fields(ME, "t").unwrap(),
        ("reply".to_string(), 0)
    );
}
