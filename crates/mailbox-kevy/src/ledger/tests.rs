use std::sync::Arc;

use kevy_embedded::{Config, Store};

use super::{LedgerFacts, LedgerVerdict};
use crate::KevyMailboxStore;

fn store() -> KevyMailboxStore {
    let s = KevyMailboxStore::new(Arc::new(Store::open(Config::default()).unwrap()));
    s.ensure_ledger_tables().unwrap();
    s
}

fn facts(mid: &str, user: &str, domain: &str, hosts: &[&str], files: &[&str]) -> LedgerFacts {
    LedgerFacts {
        message_id: mid.into(),
        user: user.into(),
        from_addr: format!("billing@{domain}"),
        from_domain: domain.into(),
        reply_to: String::new(),
        link_hosts: hosts.iter().map(|h| h.to_string()).collect(),
        attachment_sha256: files.iter().map(|f| f.to_string()).collect(),
        auth: "pass/pass/pass".into(),
        arrived: 1_700_000_000,
    }
}

#[test]
fn a_message_is_found_by_its_domain_its_links_and_its_files() {
    let st = store();
    st.record_ledger(&facts(
        "m1",
        "a@x",
        "shop.example",
        &["pay.example", "cdn.example"],
        &["aa"],
    ))
    .unwrap();
    st.record_ledger(&facts("m2", "b@x", "shop.example", &["pay.example"], &[]))
        .unwrap();
    st.record_ledger(&facts(
        "m3",
        "a@x",
        "other.example",
        &["cdn.example"],
        &["aa"],
    ))
    .unwrap();

    let mut by_domain = st.ledger_by_domain("shop.example").unwrap();
    by_domain.sort();
    assert_eq!(
        by_domain,
        [("m1".into(), "a@x".into()), ("m2".into(), "b@x".into())]
    );
    let mut by_host: Vec<String> = st
        .ledger_by_link_host("cdn.example")
        .unwrap()
        .into_iter()
        .map(|(m, _)| m)
        .collect();
    by_host.sort();
    assert_eq!(by_host, ["m1", "m3"]);
    assert_eq!(st.ledger_by_attachment("aa").unwrap().len(), 2);
    assert!(st.ledger_by_domain("nobody.example").unwrap().is_empty());
}

#[test]
fn facts_are_written_once() {
    let st = store();
    assert!(
        st.record_ledger(&facts("m1", "a@x", "shop.example", &[], &[]))
            .unwrap()
    );
    assert!(
        !st.record_ledger(&facts("m1", "a@x", "changed.example", &[], &[]))
            .unwrap()
    );
    assert!(st.ledger_by_domain("changed.example").unwrap().is_empty());
    assert_eq!(st.ledger_by_domain("shop.example").unwrap().len(), 1);
}

#[test]
fn deleting_a_message_takes_its_rows_and_verdict_out_of_every_index() {
    let st = store();
    st.record_ledger(&facts(
        "m1",
        "a@x",
        "shop.example",
        &["pay.example"],
        &["aa"],
    ))
    .unwrap();
    st.record_verdict(
        "a@x",
        "m1",
        &LedgerVerdict {
            rules: vec!["r1".into()],
            score: 2.5,
            rules_version: "v1".into(),
            judged_at: 1,
        },
    )
    .unwrap();
    st.delete_ledger("a@x", "m1").unwrap();
    assert!(st.ledger_by_domain("shop.example").unwrap().is_empty());
    assert!(st.ledger_by_link_host("pay.example").unwrap().is_empty());
    assert!(st.ledger_by_attachment("aa").unwrap().is_empty());
    assert!(st.store().hgetall(b"L:v:a@x|m1").unwrap().is_empty());
}

#[test]
fn declaring_twice_keeps_the_rows_findable() {
    let st = store();
    st.record_ledger(&facts("m1", "a@x", "shop.example", &[], &[]))
        .unwrap();
    st.ensure_ledger_tables().unwrap();
    assert_eq!(st.ledger_by_domain("shop.example").unwrap().len(), 1);
}

#[test]
fn a_relationship_keeps_its_first_time_and_counts_every_message() {
    let st = store();
    st.record_inbound_relationship("a@x", "b@y", "Bee", 100)
        .unwrap();
    st.record_inbound_relationship("a@x", "b@y", "B. Y.", 200)
        .unwrap();
    let pairs = st.store().hgetall(b"L:r:a@x:b@y").unwrap();
    let get = |f: &str| {
        pairs
            .iter()
            .find(|(k, _)| k.as_slice() == f.as_bytes())
            .map(|(_, v)| String::from_utf8_lossy(v).into_owned())
    };
    assert_eq!(get("first_at").as_deref(), Some("100"));
    assert_eq!(get("in").as_deref(), Some("2"));
    assert_eq!(get("name").as_deref(), Some("B. Y."));
}

#[test]
fn the_same_message_to_two_readers_is_two_records() {
    let st = store();
    assert!(
        st.record_ledger(&facts("m1", "a@x", "shop.example", &[], &[]))
            .unwrap()
    );
    assert!(
        st.record_ledger(&facts("m1", "b@x", "shop.example", &[], &[]))
            .unwrap()
    );
    let mut readers: Vec<String> = st
        .ledger_by_domain("shop.example")
        .unwrap()
        .into_iter()
        .map(|(_, u)| u)
        .collect();
    readers.sort();
    assert_eq!(readers, ["a@x", "b@x"]);
    st.delete_ledger("a@x", "m1").unwrap();
    assert_eq!(
        st.ledger_by_domain("shop.example").unwrap(),
        [("m1".into(), "b@x".into())]
    );
}

#[test]
fn deleting_a_thread_forgets_its_messages_in_the_ledger() {
    let st = store();
    st.ensure_thread_table();
    st.record_message_arrival(&crate::MessageArrival {
        thread_id: "t1",
        user: "a@x",
        subject: "s",
        senders_csv: "billing@shop.example",
        latest_date: 100,
        latest_preview: "p",
        category: "inbox",
        unread: true,
        is_own: false,
    })
    .unwrap();
    let wire = serde_json::json!({
        "uid": 1, "sender": "billing@shop.example", "recipients": "a@x", "subject": "s",
        "date": 1, "internal_date": 1, "size": 1, "flags": 0,
        "message_id": "m1", "in_reply_to": "", "thread_id": "t1", "modseq": 0,
    });
    let facts_row = crate::UserMessageFacts {
        blob_ref: "f1",
        uid: 1,
        flags: 0,
        modseq: 0,
    };
    st.upsert_user_message(
        "a@x",
        "t1",
        "m1",
        1,
        &serde_json::to_vec(&wire).unwrap(),
        &facts_row,
    )
    .unwrap();
    st.record_ledger(&facts("m1", "a@x", "shop.example", &["pay.example"], &[]))
        .unwrap();
    st.delete_thread("a@x", "t1").unwrap();
    assert!(st.ledger_by_domain("shop.example").unwrap().is_empty());
    assert!(st.ledger_by_link_host("pay.example").unwrap().is_empty());
}

#[test]
fn what_is_written_reads_back() {
    let st = store();
    let f = LedgerFacts {
        reply_to: "refunds@elsewhere.example".into(),
        ..facts(
            "m1",
            "a@x",
            "shop.example",
            &["pay.example", "cdn.example"],
            &["aa", "bb"],
        )
    };
    st.record_ledger(&f).unwrap();
    let back = st.ledger_facts("a@x", "m1").unwrap().unwrap();
    assert_eq!(back.message_id, "m1");
    assert_eq!(back.user, "a@x");
    assert_eq!(back.from_addr, f.from_addr);
    assert_eq!(back.from_domain, "shop.example");
    assert_eq!(back.reply_to, "refunds@elsewhere.example");
    assert_eq!(back.auth, "pass/pass/pass");
    assert_eq!(back.arrived, 1_700_000_000);
    assert_eq!(back.link_hosts, ["pay.example", "cdn.example"]);
    assert_eq!(back.attachment_sha256, ["aa", "bb"]);
    assert!(st.ledger_facts("b@x", "m1").unwrap().is_none());

    let v = LedgerVerdict {
        rules: vec!["r1".into(), "r2".into()],
        score: 4.5,
        rules_version: "lua:abc".into(),
        judged_at: 9,
    };
    st.record_verdict("a@x", "m1", &v).unwrap();
    let got = st.ledger_verdict("a@x", "m1").unwrap().unwrap();
    assert_eq!(got.rules, v.rules);
    assert_eq!(got.score, v.score);
    assert_eq!(got.rules_version, v.rules_version);
    assert_eq!(got.judged_at, v.judged_at);

    st.record_inbound_relationship("a@x", "b@y", "Bee", 100)
        .unwrap();
    assert_eq!(
        st.relationship("a@x", "b@y").unwrap(),
        Some(super::Relationship {
            first_at: 100,
            inbound: 1,
            name: "Bee".into()
        })
    );
    assert!(st.relationship("a@x", "nobody@y").unwrap().is_none());
}

#[test]
fn every_thread_of_a_reader_is_listed_in_one_stable_order() {
    let st = store();
    st.ensure_thread_table();
    for tid in ["t-c", "t-a", "t-b"] {
        st.record_message_arrival(&crate::MessageArrival {
            thread_id: tid,
            user: "a@x",
            subject: "s",
            senders_csv: "b@y",
            latest_date: 100,
            latest_preview: "p",
            category: "inbox",
            unread: true,
            is_own: false,
        })
        .unwrap();
    }
    assert_eq!(
        st.all_thread_ids_for_user("a@x").unwrap(),
        ["t-a", "t-b", "t-c"]
    );
}

#[test]
fn a_message_id_with_the_separator_in_it_is_still_found() {
    let st = store();
    st.record_ledger(&facts(
        "odd|id@x",
        "a@x",
        "shop.example",
        &["pay.example"],
        &["aa"],
    ))
    .unwrap();
    assert_eq!(
        st.ledger_by_link_host("pay.example").unwrap(),
        [("odd|id@x".into(), "a@x".into())]
    );
    assert_eq!(
        st.ledger_by_attachment("aa").unwrap(),
        [("odd|id@x".into(), "a@x".into())]
    );
}

/// A link row is an index with a retention: pruning takes it out of the
/// host's index once it is due, and the message row keeps every host.
#[test]
fn a_due_link_row_is_pruned_out_of_its_index() {
    let st = store();
    let f = facts(
        "m1",
        "a@x",
        "shop.example",
        &["pay.example", "cdn.example"],
        &[],
    );
    let now = chrono::Utc::now().timestamp();
    st.record_ledger_indexed(&f, &["pay.example".to_string()], Some(now + 100))
        .unwrap();
    st.record_ledger(&facts("m2", "a@x", "shop.example", &["pay.example"], &[]))
        .unwrap();
    let mids = |host: &str| st.ledger_by_link_host(host).unwrap();
    assert_eq!(mids("pay.example").len(), 2);
    assert!(mids("cdn.example").is_empty(), "not indexed");
    assert_eq!(
        st.prune_expired_links(now, 100).unwrap(),
        0,
        "nothing due yet"
    );
    assert_eq!(st.prune_expired_links(now + 100, 100).unwrap(), 1);
    // the row kept without a retention stays
    assert_eq!(
        mids("pay.example"),
        vec![("m2".to_string(), "a@x".to_string())]
    );
    let back = st.ledger_facts("a@x", "m1").unwrap().unwrap();
    assert_eq!(back.link_hosts, vec!["pay.example", "cdn.example"]);
}

/// Mail past the retention gets no link rows at all, and is still recorded.
#[test]
fn mail_past_the_retention_gets_no_link_rows() {
    let st = store();
    let f = facts("m1", "a@x", "shop.example", &["pay.example"], &[]);
    let past = chrono::Utc::now().timestamp() - 1;
    assert!(
        st.record_ledger_indexed(&f, &f.link_hosts, Some(past))
            .unwrap()
    );
    assert!(st.ledger_by_link_host("pay.example").unwrap().is_empty());
    assert_eq!(st.ledger_by_domain("shop.example").unwrap().len(), 1);
}
