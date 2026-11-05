//! An archive whose entry names cannot be read scores, and never holds by
//! itself: an archive that will not say what it holds proves nothing.
use mailrs_fraud::{Facts, Policy};
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

const RULE: &str = "archive-names-unreadable";

fn findings(f: &Facts<'_>) -> mailrs_fraud::finding::Findings {
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("compiles");
    let out = rules.classify(f, &Policy::default());
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    out.findings
}

#[test]
fn an_unreadable_archive_scores_three_and_holds_nothing() {
    let f = Facts {
        from: "Accounts <ap@kawamura-shoji.co.jp>",
        domain: "kawamura-shoji.co.jp",
        registrable: "kawamura-shoji.co.jp",
        subject: "invoice",
        has_unreadable_archive: true,
        ..Facts::default()
    };
    let found = findings(&f);
    let hit = found
        .iter()
        .find(|h| h.rule == RULE)
        .expect("the rule fires");
    assert_eq!(hit.score, 3.0);
    assert!(!hit.holds);
    assert!(!mailrs_inbound::holds(&found), "nothing holds it");
}

/// Not in the corpus: a sender this deployment has corresponded with for
/// years and that passes every check. The rule asks nothing about the
/// sender, so it still scores, and still holds nothing.
#[test]
fn a_known_authenticated_sender_still_scores_and_is_not_held() {
    let f = Facts {
        from: "Accounts <ap@kawamura-shoji.co.jp>",
        domain: "kawamura-shoji.co.jp",
        registrable: "kawamura-shoji.co.jp",
        dmarc: "pass",
        dkim: "pass",
        spf: "pass",
        domain_seen: 500,
        has_unreadable_archive: true,
        ..Facts::default()
    };
    let found = findings(&f);
    assert!(found.iter().any(|h| h.rule == RULE));
    assert!(!mailrs_inbound::holds(&found));
}

#[test]
fn a_message_without_one_is_not_touched() {
    let f = Facts {
        from: "Accounts <ap@kawamura-shoji.co.jp>",
        domain: "kawamura-shoji.co.jp",
        ..Facts::default()
    };
    assert!(!findings(&f).iter().any(|h| h.rule == RULE));
}
