//! A corrected message date reaches the blob, both zsets and every holder's
//! row, and nothing a reader owns: flags, uid, modseq, the file, unread
//! state and membership read back the same.

use std::collections::BTreeMap;

use crate::display_tests::{CAROLS, DAVES, store};
use crate::{KevyMailboxStore, ListThreadsFilter, MessageArrival, UserMessageFacts, keys};

const ALICE: &str = "alice@example.test";
const BOB: &str = "bob@example.test";

fn deliver(s: &KevyMailboxStore, user: &str, tid: &str, mid: &str, from: &str, date: i64) {
    let payload = format!(
        r#"{{"message_id":"{mid}","sender":"{from}","thread_id":"{tid}","internal_date":{date},"date":{date},"subject":"s","flags":0}}"#
    );
    let file = format!("{date}.{mid}.{user}");
    s.deliver_message(
        &MessageArrival {
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
        &UserMessageFacts {
            blob_ref: &file,
            uid: 7,
            flags: 0,
            modseq: 1,
        },
    )
    .unwrap();
}

/// Carol's m1 to both at 100, Dave's m2 to Alice at 200, and Bob's other
/// conversation at 150; Bob has read t1.
fn seeded() -> KevyMailboxStore {
    let s = store();
    deliver(&s, ALICE, "t1", CAROLS.mid, CAROLS.from, CAROLS.date);
    deliver(&s, BOB, "t1", CAROLS.mid, CAROLS.from, CAROLS.date);
    deliver(&s, BOB, "t2", "o1", "erin@outside.example", 150);
    deliver(&s, ALICE, "t1", DAVES.mid, DAVES.from, DAVES.date);
    s.mark_seen(BOB, "t1").unwrap();
    s
}

/// Everything a reader owns, which a redate must leave as it was.
fn owned(s: &KevyMailboxStore) -> Vec<String> {
    let mut out = Vec::new();
    for (user, mid) in [(ALICE, "m1"), (BOB, "m1"), (ALICE, "m2"), (BOB, "o1")] {
        let mut row = s
            .store()
            .hgetall(keys::user_message(user, mid).as_bytes())
            .unwrap();
        row.sort();
        out.push(format!("{user} {mid} {row:?}"));
    }
    for (user, tid) in [(ALICE, "t1"), (BOB, "t1"), (BOB, "t2")] {
        let r = s.get_thread_for_user(user, tid).unwrap().unwrap();
        out.push(format!(
            "{user} {tid} unread={} count={} archived={} starred={}",
            r.unread_count, r.count, r.archived, r.starred
        ));
    }
    let mut owners = s.thread_message_owners("t1").unwrap();
    owners.sort();
    out.push(format!("{owners:?}"));
    out
}

fn activity(s: &KevyMailboxStore, user: &str, tid: &str) -> i64 {
    s.get_thread_for_user(user, tid)
        .unwrap()
        .unwrap()
        .latest_date
}

fn blob_dates(s: &KevyMailboxStore, mid: &str) -> (i64, i64) {
    let v: serde_json::Value =
        serde_json::from_slice(&s.get_message(mid).unwrap().unwrap()).unwrap();
    (
        v["internal_date"].as_i64().unwrap(),
        v["date"].as_i64().unwrap(),
    )
}

fn score(s: &KevyMailboxStore, key: &str, mid: &str) -> i64 {
    s.store()
        .zrange(key.as_bytes(), 0, -1)
        .unwrap()
        .into_iter()
        .find(|(m, _)| m == mid.as_bytes())
        .map(|(_, sc)| sc as i64)
        .unwrap()
}

fn order(s: &KevyMailboxStore, user: &str) -> Vec<String> {
    s.list_threads_by_activity(user, &ListThreadsFilter::default(), 0, 10)
        .unwrap()
        .0
        .into_iter()
        .map(|r| r.thread_id)
        .collect()
}

/// Every key the redate could write, as stored.
fn dump(s: &KevyMailboxStore) -> String {
    let st = s.store();
    let mut out = Vec::new();
    for mid in ["m1", "m2", "o1"] {
        out.push(format!("{:?}", s.get_message(mid).unwrap()));
    }
    for tid in ["t1", "t2"] {
        out.push(format!(
            "{:?}",
            st.zrange(keys::thread_messages(tid).as_bytes(), 0, -1)
                .unwrap()
        ));
        let mut h = st.hgetall(keys::thread(tid).as_bytes()).unwrap();
        h.sort();
        out.push(format!("{h:?}"));
        for user in [ALICE, BOB] {
            let z = st.zrange(keys::thread_user_messages(user, tid).as_bytes(), 0, -1);
            out.push(format!("{:?}", z.unwrap()));
            let mut h = st.hgetall(keys::thread_user(user, tid).as_bytes()).unwrap();
            h.sort();
            out.push(format!("{h:?}"));
        }
    }
    out.join("\n")
}

#[test]
fn a_dry_run_reports_every_holder_and_writes_nothing() {
    let s = seeded();
    let before = owned(&s);
    let at = dump(&s);
    let dates = BTreeMap::from([("m1".to_string(), 300)]);
    let r = s.redate_messages("t1", &dates, true).unwrap();
    assert_eq!(dump(&s), at, "a dry run wrote");
    let rows: Vec<_> = r
        .rows
        .iter()
        .map(|r| (r.user.as_str(), r.from, r.to))
        .collect();
    assert_eq!(rows, vec![(ALICE, 200, 300), (BOB, 100, 300)]);
    assert_eq!((r.shared_from, r.shared_to), (200, 300));
    assert_eq!(blob_dates(&s, "m1"), (100, 100));
    assert_eq!(owned(&s), before);
}

#[test]
fn a_redate_moves_every_holders_row_and_nothing_they_own() {
    let s = seeded();
    let before = owned(&s);
    assert_eq!(order(&s, BOB), ["t2", "t1"]);
    let dates = BTreeMap::from([("m1".to_string(), 300)]);
    s.redate_messages("t1", &dates, false).unwrap();

    assert_eq!(blob_dates(&s, "m1"), (300, 300));
    assert_eq!(score(&s, &keys::thread_messages("t1"), "m1"), 300);
    for user in [ALICE, BOB] {
        assert_eq!(
            score(&s, &keys::thread_user_messages(user, "t1"), "m1"),
            300
        );
        assert_eq!(activity(&s, user, "t1"), 300, "{user}");
    }
    assert_eq!(s.get_thread("t1").unwrap().unwrap().latest_date, 300);
    assert_eq!(order(&s, BOB), ["t1", "t2"], "Bob's list re-sorts");
    assert_eq!(owned(&s), before, "flags, unread or membership moved");

    // the same correction again finds every row already right
    let at = dump(&s);
    let again = s.redate_messages("t1", &BTreeMap::new(), false).unwrap();
    assert!(again.rows.iter().all(|r| r.from == r.to), "{again:?}");
    assert_eq!(again.shared_from, again.shared_to);
    assert_eq!(dump(&s), at, "a second run wrote");
}

#[test]
fn an_earlier_date_lets_the_next_newest_message_lead() {
    let s = seeded();
    let dates = BTreeMap::from([("m2".to_string(), 50)]);
    let r = s.redate_messages("t1", &dates, false).unwrap();
    // Alice's newest is now Carol's at 100; Bob never held m2
    assert_eq!(activity(&s, ALICE, "t1"), 100);
    assert_eq!(activity(&s, BOB, "t1"), 100);
    assert_eq!((r.shared_from, r.shared_to), (200, 100));
    assert_eq!(
        score(&s, &keys::thread_user_messages(ALICE, "t1"), "m2"),
        50
    );
}

#[test]
fn what_readers_own_survives_a_redate_and_a_read_changes_it() {
    let s = seeded();
    let before = s.owned_in("t1").unwrap();
    assert_eq!((before.rows, before.messages), (2, 3));
    // delivered with no flags; Bob's read clears only his row's column
    assert_eq!((before.unseen_messages, before.unread_rows), (3, 1));
    let dates = BTreeMap::from([("m1".to_string(), 300)]);
    s.redate_messages("t1", &dates, false).unwrap();
    assert_eq!(s.owned_in("t1").unwrap(), before);
    s.mark_seen(ALICE, "t1").unwrap();
    let read = s.owned_in("t1").unwrap();
    assert_ne!(read.digest, before.digest, "a read went unseen");
    assert_eq!(
        read.unread_rows + 1,
        before.unread_rows,
        "{before:?} {read:?}"
    );
}
