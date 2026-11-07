//! Japanese accounts and account changes: shadow only, never convicting on
//! an account alone, and never carrying the full number into a finding.

use kevy_client::Connection;
use mailrs_core_sidestate::families::{contact_sent, payment_accounts};
use mailrs_fraud::payment::find;
use mailrs_fraud::relation::Relation;
use mailrs_fraud::{Facts, Policy};
use mailrs_fraud_lua::stored::relation::Counted;
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

const ME: &str = "me@golia.example";
const FROM: &str = "Billing <billing@supplier.example>";
const ACCOUNT: &str = "振込先：三菱UFJ銀行 渋谷支店 普通 1234567";

fn base() -> Facts<'static> {
    Facts {
        from: FROM,
        domain: "supplier.example",
        registrable: "supplier.example",
        subject: "請求書",
        domain_seen: 40,
        spf: "pass",
        dkim: "pass",
        dmarc: "pass",
        ..Facts::default()
    }
}

fn policy() -> Policy {
    Policy {
        our_domains: vec!["golia.jp".into()],
        ..Policy::default()
    }
}

#[test]
fn findings_carry_the_masked_account_and_never_the_number() {
    let facts = Facts {
        payment: find(ACCOUNT),
        relation: Relation {
            first_contact: true,
            ..Relation::default()
        },
        ..base()
    };
    let out = Rules::compile(DEFAULT_SOURCE)
        .unwrap()
        .classify(&facts, &policy());
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    let hit = out
        .shadow
        .iter()
        .find(|f| f.rule == "bec-jp-account-from-a-stranger")
        .expect("the shadow rule fires");
    assert_eq!(hit.params["account"], "三菱UFJ銀行 渋谷支店 普通 ****567");
    assert!(!out.findings.has("bec-jp-account-from-a-stranger"));
    let everything = format!("{:?} {:?}", out.findings, out.shadow);
    assert!(!everything.contains("1234567"), "{everything}");
}

#[test]
fn an_account_alone_scores_and_holds_nothing() {
    let mut rules = Rules::compile(DEFAULT_SOURCE).unwrap();
    let without = rules.classify(&base(), &policy());
    for text in [ACCOUNT, "ゆうちょ銀行 記号 10120 番号 12345671"] {
        let with = rules.classify(
            &Facts {
                payment: find(text),
                ..base()
            },
            &policy(),
        );
        assert!(with.errors.is_empty(), "{:?}", with.errors);
        assert_eq!(with.findings.rules(), without.findings.rules(), "{text}");
        assert_eq!(with.findings.score(), without.findings.score(), "{text}");
        assert_eq!(
            with.findings.hold_worthy(),
            without.findings.hold_worthy(),
            "{text}"
        );
        assert!(with.shadow.is_empty(), "{text}: {:?}", with.shadow);
    }
}

#[test]
fn our_own_invoice_to_a_first_contact_is_not_a_stranger_asking() {
    let facts = Facts {
        from: "経理 <billing@golia.jp>",
        domain: "golia.jp",
        registrable: "golia.jp",
        payment: find(ACCOUNT),
        relation: Relation {
            first_contact: true,
            account_changed: true,
            ..Relation::default()
        },
        ..base()
    };
    let out = Rules::compile(DEFAULT_SOURCE)
        .unwrap()
        .classify(&facts, &policy());
    assert!(out.shadow.is_empty(), "{:?}", out.shadow);
}

/// The change is read from the per-sender history on a re-judgement, as
/// delivery wrote it: an account given before is no change, another is.
#[test]
fn a_stored_message_with_another_account_reads_as_a_change() {
    let mut c = Connection::connect("mem://jp-account-change").unwrap();
    contact_sent::set_counts(&mut c, ME, "billing@supplier.example", 3, 0).unwrap();
    let before = find("三菱UFJ銀行 渋谷支店 普通 7654321").account_keys;
    payment_accounts::record(&mut c, ME, FROM, &before, 100, "<old@supplier>").unwrap();
    let mut hist = Some(c);
    for (account, changed) in [("1234567", true), ("7654321", false)] {
        let raw = format!(
            "From: {FROM}\r\nTo: {ME}\r\nSubject: invoice\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n三菱UFJ銀行 渋谷支店 普通 {account}\r\n"
        );
        let mut seen: Option<Facts<'static>> = None;
        let mut rules = Rules::compile(DEFAULT_SOURCE).unwrap();
        let judged = mailrs_fraud_lua::stored::judge_with(
            raw.as_bytes(),
            Some((ME, Counted::After)),
            None,
            Default::default(),
            &policy(),
            &mut hist,
            &mut |facts, p| {
                seen = Some(Facts {
                    relation: facts.relation.clone(),
                    ..Facts::default()
                });
                let ev = rules.classify(facts, p);
                Ok(mailrs_fraud_lua::Scan {
                    findings: ev.findings,
                    version: rules.version().to_string(),
                    shadow: ev.shadow.iter().map(|f| f.rule.clone()).collect(),
                })
            },
        )
        .unwrap();
        let relation = seen.expect("judged").relation;
        assert!(!relation.first_contact, "{account}");
        assert_eq!(relation.account_changed, changed, "{account}");
        // shadow: whatever else the message scores, the change adds nothing
        assert!(!judged.findings.has("bec-account-changed"), "{account}");
    }
}
