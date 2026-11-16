//! The fact ledger, alike in both stores: the same mail, arriving through
//! each core's intake, is found by the same sender-domain and link-host
//! lookups, subdomains included, with a verdict recorded beside each.

use super::harness;
use super::intake::{Spools, envelope};
use super::{ALICE, accounts};

fn letter(n: u32, from: &str, body: &str) -> Vec<u8> {
    format!(
        "Received: from mx.example (mx.example [192.0.2.1])\r\n\tby mail.contract.test; Mon, 2 Nov 2026 00:00:0{n} +0000\r\n\
         From: <{from}>\r\nTo: <{ALICE}>\r\nSubject: ledger {n}\r\n\
         Message-ID: <ledger-{n}@contract.test>\r\nDate: Mon, 2 Nov 2026 00:00:0{n} +0000\r\n\
         Content-Type: text/html\r\n\r\n<p>{body}</p>\r\n"
    )
    .into_bytes()
}

fn letters() -> Vec<Vec<u8>> {
    vec![
        letter(
            1,
            "news@mail.shop.example",
            r#"<a href="https://pay-here.example/a">pay</a> <a href="https://cdn.pay-here.example/b">img</a>"#,
        ),
        letter(
            2,
            "billing@shop.example",
            r#"<a href="https://other.example/">x</a>"#,
        ),
        letter(
            3,
            "friend@elsewhere.example",
            r#"<a href="https://pay-here.example/c">y</a>"#,
        ),
    ]
}

fn ids(mut found: Vec<(String, String)>) -> Vec<String> {
    found.sort();
    found.dedup();
    found
        .into_iter()
        .map(|(mid, user)| {
            assert_eq!(user, ALICE);
            mid
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn both_stores_find_the_same_mail_in_the_ledger() {
    let c = harness::start_on("mem://ledger-fastcore", "mem://ledger-pg-core").await;
    let mut d = harness::Diffs::default();
    accounts::seed(&c, &mut d).await;
    assert!(d.0.is_empty(), "{}", d.0.join("\n\n"));
    let spools = Spools::new(&c);
    for l in letters() {
        spools.arrive(&c, &envelope(None), &l).await;
    }
    let m = |n: u32| format!("ledger-{n}@contract.test");
    let asked: [(&str, &str, Vec<String>); 5] = [
        ("domain", "shop.example", vec![m(1), m(2)]),
        ("domain", "mail.shop.example", vec![m(1)]),
        ("link", "pay-here.example", vec![m(1), m(3)]),
        ("link", "cdn.pay-here.example", vec![m(1)]),
        ("link", "other.example", vec![m(2)]),
    ];
    let fc = &c.fastcore_state.mailbox;
    let pg = &c.pg_core_state.mailbox;
    for (axis, value, want) in asked {
        let reg = mailrs_reputation::registrable(value);
        let (kevy, sql) = match axis {
            "domain" => (
                fc.ledger_by_domain_tree(value, &reg).expect("kevy ledger"),
                pg.ledger_by_domain_tree(value, &reg)
                    .await
                    .expect("sql ledger"),
            ),
            _ => (
                fc.ledger_by_link_tree(value, &reg).expect("kevy ledger"),
                pg.ledger_by_link_tree(value, &reg)
                    .await
                    .expect("sql ledger"),
            ),
        };
        assert_eq!(ids(kevy), want, "fastcore, {axis} {value}");
        assert_eq!(ids(sql), want, "pg-core, {axis} {value}");
    }
    for n in 1..=3 {
        let kevy = fc.ledger_verdict(ALICE, &m(n)).expect("kevy verdict");
        let sql = pg.ledger_verdict(ALICE, &m(n)).await.expect("sql verdict");
        let (kevy, sql) = (
            kevy.expect("fastcore recorded a verdict"),
            sql.expect("pg-core recorded a verdict"),
        );
        assert_eq!(
            (kevy.rules, kevy.holds, kevy.judged_at),
            (sql.rules, sql.holds, sql.judged_at),
            "verdict of letter {n}"
        );
    }
    // a deleted conversation is forgotten by the ledger on both
    for (name, client) in [("fastcore", &c.fastcore), ("pg-core", &c.pg_core)] {
        client
            .delete_thread(ALICE, &m(3))
            .await
            .unwrap_or_else(|e| panic!("{name}: delete: {e:?}"));
    }
    let reg = mailrs_reputation::registrable("pay-here.example");
    let kevy = fc
        .ledger_by_link_tree("pay-here.example", &reg)
        .expect("kevy ledger");
    let sql = pg
        .ledger_by_link_tree("pay-here.example", &reg)
        .await
        .expect("sql ledger");
    assert_eq!(ids(kevy), vec![m(1)], "fastcore after the delete");
    assert_eq!(ids(sql), vec![m(1)], "pg-core after the delete");
}
