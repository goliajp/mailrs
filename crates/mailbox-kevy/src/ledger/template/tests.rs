use std::sync::Arc;

use kevy_embedded::{Config, Store};
use mailrs_report::Template;

use super::SEGMENT_DAYS;
use crate::KevyMailboxStore;
use crate::ledger::LedgerFacts;

fn store() -> KevyMailboxStore {
    let s = KevyMailboxStore::new(Arc::new(Store::open(Config::default()).unwrap()));
    s.ensure_ledger_tables().unwrap();
    s
}

fn t(h: u64) -> Template {
    Template {
        simhash: h,
        link_domains: vec!["evil.example".into()],
        attachments: vec![],
        sender_domain: "s.example".into(),
    }
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

#[test]
fn a_fingerprint_is_recorded_once_per_message_and_read_back() {
    let st = store();
    assert!(st.record_template("m1", &t(0xabcd), now()).unwrap());
    assert!(!st.record_template("m1", &t(0xffff), now()).unwrap());
    let got = st.template_of("m1").unwrap().unwrap();
    assert_eq!(got.template, t(0xabcd));
    assert_eq!(st.template_of("missing").unwrap(), None);
}

#[test]
fn candidates_share_a_piece_and_arrived_within_the_window() {
    let st = store();
    let base: u64 = 0x1111_2222_3333_4444;
    st.record_template("near", &t(base ^ 0b101), now()).unwrap();
    // every piece changed: cannot be within three bits
    st.record_template("far", &t(!base), now()).unwrap();
    let mut c = st.template_candidates(base, now() - 3600, 100).unwrap();
    c.sort();
    assert_eq!(c, vec!["near".to_string()]);
    assert!(
        st.template_candidates(base, now() + 3600, 100)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_message_older_than_the_window_is_not_findable_but_keeps_its_fingerprint() {
    let st = store();
    let old = now() - (SEGMENT_DAYS + 1) * 86_400;
    st.record_template("old", &t(7), old).unwrap();
    assert!(st.template_candidates(7, 0, 100).unwrap().is_empty());
    assert!(st.template_of("old").unwrap().is_some());
}

#[test]
fn pruning_removes_the_index_rows_and_keeps_the_fingerprint() {
    let st = store();
    let a = now() - 10 * 86_400;
    st.record_template("a", &t(9), a).unwrap();
    st.record_template("b", &t(9), now()).unwrap();
    assert_eq!(st.prune_template_segments(now() - 86_400, 100).unwrap(), 1);
    assert_eq!(
        st.template_candidates(9, 0, 100).unwrap(),
        vec!["b".to_string()]
    );
    assert!(st.template_of("a").unwrap().is_some());
    assert_eq!(st.prune_template_segments(now() - 86_400, 100).unwrap(), 0);
}

#[test]
fn the_readers_of_a_message_are_found_from_the_ledger() {
    let st = store();
    for user in ["a@x", "b@x"] {
        st.record_ledger(&LedgerFacts {
            message_id: "m".into(),
            user: user.into(),
            arrived: now(),
            ..Default::default()
        })
        .unwrap();
    }
    let mut r = st.ledger_readers("m").unwrap();
    r.sort();
    assert_eq!(r, vec!["a@x".to_string(), "b@x".to_string()]);
    assert!(st.ledger_readers("none").unwrap().is_empty());
}

#[test]
fn the_footprint_counts_findable_rows_and_their_bytes() {
    let st = store();
    st.record_template("m1", &t(1), now()).unwrap();
    st.record_template("m2", &t(2), now()).unwrap();
    let f = st.template_footprint(100).unwrap();
    assert_eq!((f.fingerprints, f.segment_rows, f.capped), (2, 2, false));
    assert!(f.fingerprint_bytes > 0 && f.segment_bytes > 0);
}
