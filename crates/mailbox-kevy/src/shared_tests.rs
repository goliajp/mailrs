use std::sync::Arc;

use kevy_embedded::{Config, Store};
use mailrs_mailbox::shared_reply::Claim;

use crate::{KevyMailboxStore, MessageArrival};

fn store() -> KevyMailboxStore {
    let s = KevyMailboxStore::new(Arc::new(
        Store::open(Config::default()).expect("open in-memory kevy"),
    ));
    s.ensure_thread_table();
    s
}

fn arrive(st: &KevyMailboxStore, user: &str, tid: &str, at: i64) {
    st.record_message_arrival(&MessageArrival {
        thread_id: tid,
        user,
        subject: "s",
        senders_csv: "c@outside.test",
        latest_date: at,
        latest_preview: "p",
        category: "inbox",
        unread: true,
        is_own: false,
    })
    .unwrap();
}

#[test]
fn assigned_to_me_is_read_off_the_declared_axis() {
    let st = store();
    for (tid, at) in [("t1", 10), ("t2", 20), ("t3", 30)] {
        arrive(&st, "a@x", tid, at);
    }
    st.set_shared_copy("a@x", "t1", "1:1", "a@x").unwrap();
    st.set_shared_copy("a@x", "t2", "1:2", "b@x").unwrap();
    st.set_shared_copy("a@x", "t3", "1:3", "a@x").unwrap();
    assert_eq!(
        st.list_thread_ids_assigned_via_table("a@x", "a@x", None, 10)
            .unwrap(),
        ["t3", "t1"]
    );
    assert_eq!(
        st.list_thread_ids_assigned_via_table("a@x", "a@x", Some(29), 10)
            .unwrap(),
        ["t1"]
    );
    st.set_assignee("a@x", "t3", "").unwrap();
    assert_eq!(
        st.list_thread_ids_assigned_via_table("a@x", "a@x", None, 10)
            .unwrap(),
        ["t1"]
    );
    assert_eq!(
        st.shared_fields("a@x", "t2").unwrap(),
        ("1:2".to_string(), "b@x".to_string())
    );
    assert_eq!(
        st.shared_fields("a@x", "nope").unwrap(),
        (String::new(), String::new())
    );
}

#[test]
fn two_claims_raced_on_the_same_count_let_exactly_one_through() {
    let st = Arc::new(store());
    let mut handles = Vec::new();
    for i in 0..8 {
        let st = st.clone();
        handles.push(std::thread::spawn(move || {
            st.claim_reply("1:1", &format!("m{i}@x"), 0, false, 100 + i)
                .unwrap()
        }));
    }
    let won = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .filter(|c| matches!(c, Claim::Won { .. }))
        .count();
    assert_eq!(won, 1);
    assert_eq!(st.reply_state("1:1").unwrap().seq, 1);
}

#[test]
fn an_undone_reply_releases_its_claim() {
    let st = store();
    let Claim::Won { next } = st.claim_reply("1:9", "a@x", 0, false, 5).unwrap() else {
        panic!("first claim lost");
    };
    assert!(matches!(
        st.claim_reply("1:9", "b@x", 0, false, 6).unwrap(),
        Claim::Lost { .. }
    ));
    assert!(st.release_reply("1:9", &next.claim_id).unwrap());
    assert!(!st.release_reply("1:9", &next.claim_id).unwrap());
    assert!(matches!(
        st.claim_reply("1:9", "b@x", 0, false, 7).unwrap(),
        Claim::Won { .. }
    ));
}

#[test]
fn a_group_purge_takes_its_reply_claims_and_no_other_groups() {
    let st = store();
    for tag in ["4:1", "4:2", "40:1", "5:1"] {
        assert!(matches!(
            st.claim_reply(tag, "a@x", 0, false, 1).unwrap(),
            Claim::Won { .. }
        ));
    }
    assert_eq!(st.purge_group_replies(4, true).unwrap(), 2);
    assert_eq!(
        st.reply_state("4:1").unwrap().seq,
        1,
        "a dry run removed it"
    );
    assert_eq!(st.purge_group_replies(4, false).unwrap(), 2);
    assert_eq!(st.reply_state("4:1").unwrap().seq, 0);
    assert_eq!(st.reply_state("4:2").unwrap().seq, 0);
    assert_eq!(
        st.reply_state("40:1").unwrap().seq,
        1,
        "group 40 is not group 4"
    );
    assert_eq!(st.reply_state("5:1").unwrap().seq, 1);
    assert_eq!(st.purge_group_replies(4, false).unwrap(), 0);
}
