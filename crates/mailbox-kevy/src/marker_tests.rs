//! The index markers through the store's own mutations: set writes the
//! marker with the column's value, clear removes it, and the marker
//! indexes answer what the full-row indexes they replace answer.

use std::collections::HashMap;
use std::sync::Arc;

use kevy_embedded::{Config, IndexValue, Store};
use mailrs_mailbox::section::MessageFacts;

use crate::thread_row::MARKED;

/// The marker index, named directly rather than through the read switch.
fn marker_index(col: &str) -> String {
    crate::thread_row::marker_index(col).expect("a marked column")
}
use crate::{KevyMailboxStore, MessageArrival, SectionArrival, keys};

const U: &str = "me@x.test";

fn store() -> KevyMailboxStore {
    let s = KevyMailboxStore::new(Arc::new(
        Store::open(Config::default()).expect("open in-memory kevy"),
    ));
    s.ensure_thread_table();
    s.ensure_admin_indexes();
    s
}

fn arrive(st: &KevyMailboxStore, user: &str, tid: &str, at: i64, own: bool, csv: &str) {
    st.record_message_arrival(&MessageArrival {
        thread_id: tid,
        user,
        subject: "s",
        senders_csv: csv,
        latest_date: at,
        latest_preview: "p",
        category: "inbox",
        unread: !own,
        is_own: own,
    })
    .unwrap();
}

fn fields(st: &KevyMailboxStore, user: &str, tid: &str) -> HashMap<String, String> {
    st.store()
        .hgetall(keys::thread_user(user, tid).as_bytes())
        .unwrap()
        .into_iter()
        .map(|(f, v)| {
            (
                String::from_utf8_lossy(&f).into_owned(),
                String::from_utf8_lossy(&v).into_owned(),
            )
        })
        .collect()
}

/// The column holds `want`, and its marker holds the same value when
/// `want` is non-zero and is absent — not "0" — when it is zero.
#[track_caller]
fn assert_marked(st: &KevyMailboxStore, tid: &str, column: &str, want: &str) {
    let row = fields(st, U, tid);
    assert_eq!(row.get(column).map(String::as_str), Some(want), "{column}");
    let marker = format!("ix_{column}");
    match want == "0" {
        true => assert!(
            !row.contains_key(&marker),
            "{marker} must be removed when {column} is 0, found {:?}",
            row.get(&marker)
        ),
        false => assert_eq!(
            row.get(&marker).map(String::as_str),
            Some(want),
            "{marker} must carry {column}'s value"
        ),
    }
}

#[test]
fn the_curated_flags_set_and_clear_their_markers() {
    let st = store();
    arrive(&st, U, "t", 100, false, "bob@y.com");
    for col in ["starred", "archived", "pinned", "has_action"] {
        assert_marked(&st, "t", col, "0");
    }

    assert!(st.set_starred(U, "t", true).unwrap());
    assert_marked(&st, "t", "starred", "1");
    assert!(st.set_starred(U, "t", false).unwrap());
    assert_marked(&st, "t", "starred", "0");

    assert!(st.set_archived(U, "t", true).unwrap());
    assert_marked(&st, "t", "archived", "1");
    assert!(st.set_archived(U, "t", false).unwrap());
    assert_marked(&st, "t", "archived", "0");

    assert!(st.set_pinned(U, "t", true).unwrap());
    assert_marked(&st, "t", "pinned", "1");
    assert!(st.set_pinned(U, "t", false).unwrap());
    assert_marked(&st, "t", "pinned", "0");

    assert!(st.set_has_action(U, "t", true).unwrap());
    assert_marked(&st, "t", "has_action", "1");
    assert!(st.set_has_action(U, "t", false).unwrap());
    assert_marked(&st, "t", "has_action", "0");

    // filing a conversation un-archives it, and that clears the marker too
    assert!(st.set_archived(U, "t", true).unwrap());
    assert!(st.set_bucket(U, "t", keys::Bucket::Inbox).unwrap());
    assert_marked(&st, "t", "archived", "0");

    // a column without a marker gets none
    assert!(st.set_quarantined(U, "t", true).unwrap());
    assert!(!fields(&st, U, "t").contains_key("ix_quarantined"));
}

#[test]
fn unread_follows_arrival_read_and_unread() {
    let st = store();
    arrive(&st, U, "t", 100, false, "bob@y.com");
    assert_marked(&st, "t", "unread", "1");
    assert!(st.mark_seen(U, "t").unwrap());
    assert_marked(&st, "t", "unread", "0");
    assert!(st.mark_unread(U, "t").unwrap());
    assert_marked(&st, "t", "unread", "1");
    assert_eq!(st.mark_all_seen(U).unwrap(), 1);
    assert_marked(&st, "t", "unread", "0");
}

/// A held conversation keeps its unread mail: the badge does not count it
/// and "mark all as read" does not take it, and a release counts it again.
#[test]
fn held_unread_is_out_of_the_badge_and_out_of_mark_all_seen() {
    let st = store();
    arrive(&st, U, "held", 100, false, "bob@y.com");
    arrive(&st, U, "live", 200, false, "bob@y.com");
    assert!(st.set_quarantined(U, "held", true).unwrap());
    assert_eq!(
        st.count_flag_non_junk(U, "unread").unwrap(),
        1,
        "the badge counted held mail"
    );
    assert_eq!(
        st.count_flag_in_buckets(U, "unread", &["inbox"]).unwrap(),
        1
    );

    assert_eq!(st.mark_all_seen(U).unwrap(), 1);
    assert_marked(&st, "held", "unread", "1");
    assert_marked(&st, "live", "unread", "0");

    assert!(st.set_quarantined(U, "held", false).unwrap());
    assert_eq!(st.count_flag_non_junk(U, "unread").unwrap(), 1);
}

#[test]
fn trash_sets_and_restore_clears_both_markers() {
    let st = store();
    arrive(&st, U, "t", 100, false, "bob@y.com");
    assert!(st.set_trashed(U, "t", true, 5_000).unwrap());
    assert_marked(&st, "t", "trashed", "1");
    assert_marked(&st, "t", "trashed_at", "5000");
    assert_eq!(
        st.trashed_before(6_000, 10).unwrap(),
        vec![(U.to_string(), "t".to_string())],
        "the purge reads the marker index"
    );
    assert!(st.set_trashed(U, "t", false, 6_000).unwrap());
    assert_marked(&st, "t", "trashed", "0");
    assert_marked(&st, "t", "trashed_at", "0");
    assert!(st.trashed_before(i64::MAX, 10).unwrap().is_empty());
}

#[test]
fn snooze_unsnooze_and_expiry_keep_the_markers() {
    let st = store();
    arrive(&st, U, "t", 100, false, "bob@y.com");
    assert!(st.set_snoozed(U, "t", 9_000).unwrap());
    assert_marked(&st, "t", "snoozed_until", "9000");
    assert_marked(&st, "t", "archived", "1");
    assert!(st.set_snoozed(U, "t", 0).unwrap());
    assert_marked(&st, "t", "snoozed_until", "0");
    assert_marked(&st, "t", "archived", "0");

    // and the timer's own path: due at 50, woken at 100
    assert!(st.set_snoozed(U, "t", 50).unwrap());
    assert_eq!(st.wake_snoozed(40).unwrap(), 0, "not due yet");
    assert_eq!(
        st.wake_snoozed(100).unwrap(),
        1,
        "found on the marker index"
    );
    assert_marked(&st, "t", "snoozed_until", "0");
    assert_marked(&st, "t", "archived", "0");
    assert_eq!(st.wake_snoozed(100).unwrap(), 0, "and nothing left to wake");
}

#[test]
fn awaiting_since_is_marked_while_waiting_and_cleared_by_the_reply() {
    let st = store();
    let section = |at: i64, own: bool, facts: MessageFacts| {
        arrive(&st, U, "t", at, own, "bob@y.com");
        st.record_section_arrival(&SectionArrival {
            user: U,
            thread_id: "t",
            at,
            is_own: own,
            facts,
            from_addr: "bob@y.com",
        })
        .unwrap();
    };
    let mine = MessageFacts {
        to_others: true,
        ..MessageFacts::default()
    };
    let person = MessageFacts {
        direct: true,
        from_person: true,
        ..MessageFacts::default()
    };
    section(200, true, mine);
    assert_marked(&st, "t", "awaiting_since", "200");
    assert_eq!(
        st.list_awaiting_via_index(U, None).unwrap(),
        vec![("t".to_string(), 200)],
        "the follow-up list reads the marker index"
    );
    section(300, false, person);
    assert_marked(&st, "t", "awaiting_since", "0");
    assert!(st.list_awaiting_via_index(U, None).unwrap().is_empty());
}

#[test]
fn is_sender_follows_the_row_writer_both_ways() {
    let st = store();
    // a message this user sent
    arrive(&st, U, "t", 100, true, U);
    assert_marked(&st, "t", "is_sender", "1");
    // the same conversation, rewritten from a row that says they did not
    let mut row = st.get_thread_for_user(U, "t").unwrap().unwrap();
    row.senders_csv = "bob@y.com".into();
    st.upsert_thread(U, &row).unwrap();
    assert_marked(&st, "t", "is_sender", "0");
}

/// A spread of rows over two users, with every marked column set on some.
fn varied(st: &KevyMailboxStore) {
    for user in [U, "other@x.test"] {
        for i in 0..8 {
            let tid = format!("t{i}");
            arrive(st, user, &tid, 100 + i, i % 3 == 0, "bob@y.com, me@x.test");
            if i % 2 == 0 {
                st.set_starred(user, &tid, true).unwrap();
            }
            if i % 3 == 1 {
                st.set_archived(user, &tid, true).unwrap();
            }
            if i % 4 == 1 {
                st.set_pinned(user, &tid, true).unwrap();
                st.set_has_action(user, &tid, true).unwrap();
            }
            if i == 5 {
                st.set_trashed(user, &tid, true, 1_000).unwrap();
            }
            if i == 6 {
                st.set_snoozed(user, &tid, 2_000).unwrap();
            }
            if i % 2 == 1 {
                st.mark_seen(user, &tid).unwrap();
            }
        }
    }
}

/// Rows written before the markers existed: the rewrite fills them in,
/// a second pass changes nothing, and the check reads zero after.
#[test]
fn the_rewrite_converges_and_the_check_reads_zero() {
    let st = store();
    varied(&st);
    let markers: Vec<&[u8]> = MARKED.iter().map(|m| m.marker.as_bytes()).collect();
    let pattern = format!("{}*", String::from_utf8_lossy(keys::THREAD_USER_PREFIX));
    let all = st.store().keys(Some(pattern.as_bytes()), None);
    for key in &all {
        st.store().hdel(key, &markers).unwrap();
    }
    // and one left over, and one holding the wrong value
    let stray = keys::thread_user(U, "t1");
    st.store()
        .hset(
            stray.as_bytes(),
            &[(b"ix_starred".as_slice(), b"1".as_slice())],
        )
        .unwrap();
    let wrong = keys::thread_user(U, "t6");
    st.store()
        .hset(
            wrong.as_bytes(),
            &[(b"ix_snoozed_until".as_slice(), b"7".as_slice())],
        )
        .unwrap();

    let (rows, before) = st.threaduser_marker_check().unwrap();
    assert_eq!(rows, all.len());
    let total: usize = before.iter().map(|c| c.mismatches).sum();
    assert!(
        total > 0,
        "the check must see the missing markers: {before:?}"
    );
    let starred = before.iter().find(|c| c.column == "starred").unwrap();
    assert_eq!(starred.mismatches, starred.rows_set + 1, "{starred:?}");
    assert!(starred.samples.contains(&stray));

    // paged, two rows at a time
    let mut offset = 0;
    let mut changed = 0;
    loop {
        let page = st.rewrite_threaduser_markers(offset, 2).unwrap();
        assert_eq!(page.total_rows, all.len());
        assert_eq!(page.walked, page.changed + page.already_correct);
        changed += page.changed;
        offset += page.walked;
        if page.walked == 0 {
            break;
        }
    }
    assert_eq!(offset, all.len(), "every row walked");
    assert!(changed > 0);

    let again = st.rewrite_threaduser_markers(0, usize::MAX).unwrap();
    assert_eq!(again.walked, all.len());
    assert_eq!(again.changed, 0, "a second pass writes nothing");
    assert_eq!(again.already_correct, all.len());

    let (_, after) = st.threaduser_marker_check().unwrap();
    for c in &after {
        assert_eq!(c.mismatches, 0, "{c:?}");
        assert_eq!(c.markers_present, c.rows_set, "{c:?}");
        assert_eq!(c.index_entries, c.markers_present as u64, "{c:?}");
    }
    assert!(after.iter().any(|c| c.rows_set > 0));
    assert!(!fields(&st, U, "t1").contains_key("ix_starred"));
    assert_eq!(
        fields(&st, U, "t6")
            .get("ix_snoozed_until")
            .map(String::as_str),
        Some("2000")
    );
}

/// A flag, the predicates stacked on it, and the threads it must list.
type Case<'a> = (&'a str, &'a [(&'a str, &'a str)], Vec<String>);

/// The marker indexes list exactly the rows that carry the flag: the
/// expectations are the rows themselves, not one index checked against
/// another.
#[test]
fn flag_reads_list_the_rows_that_carry_the_flag() {
    let st = store();
    varied(&st);
    let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let cases: [Case; 5] = [
        ("starred", &[], ids(&["t6", "t4", "t2", "t0"])),
        ("archived", &[], ids(&["t7", "t6", "t4", "t1"])),
        ("unread", &[], ids(&["t4", "t2"])),
        ("trashed", &[], ids(&["t5"])),
        (
            "starred",
            &[("archived", "0"), ("trashed", "0")],
            ids(&["t2", "t0"]),
        ),
    ];
    {
        let name = marker_index;
        for (flag, extra, want) in &cases {
            let ix = name(flag);
            let got = st
                .list_thread_ids_on_flag_index(&ix, U, extra, 100, 0, None)
                .unwrap();
            assert_eq!(&got, want, "{ix} {extra:?}");
            assert_eq!(
                st.count_thread_ids_on_flag_index(&ix, U, extra).unwrap(),
                want.len(),
                "{ix} {extra:?} count"
            );
        }
        for (col, tid) in [("snoozed_until", "t6"), ("trashed_at", "t5")] {
            let ix = name(col);
            let (rows, _) = st
                .store()
                .idx_query(
                    ix.as_bytes(),
                    &IndexValue::I64(1),
                    &IndexValue::I64(i64::MAX),
                    None,
                    100,
                )
                .unwrap();
            let mut got: Vec<Vec<u8>> = rows.into_iter().map(|(k, _)| k).collect();
            got.sort();
            let mut want = vec![
                keys::thread_user("other@x.test", tid).into_bytes(),
                keys::thread_user(U, tid).into_bytes(),
            ];
            want.sort();
            assert_eq!(got, want, "{ix}");
        }
    }
}
