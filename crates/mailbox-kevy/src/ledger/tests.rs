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
        "m1",
        &LedgerVerdict {
            rules: vec!["r1".into()],
            score: 2.5,
            rules_version: "v1".into(),
            judged_at: 1,
        },
    )
    .unwrap();
    st.delete_ledger("m1").unwrap();
    assert!(st.ledger_by_domain("shop.example").unwrap().is_empty());
    assert!(st.ledger_by_link_host("pay.example").unwrap().is_empty());
    assert!(st.ledger_by_attachment("aa").unwrap().is_empty());
    assert!(st.store().hgetall(b"L:v:m1").unwrap().is_empty());
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
