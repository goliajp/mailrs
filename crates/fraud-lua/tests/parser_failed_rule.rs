//! A message the parse worker failed on scores, and never holds by
//! itself: the crash is a fact about the bytes, not about the sender.
use mailrs_fraud::{Facts, Policy};
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

const RULE: &str = "parser-failed-on-message";

fn findings(f: &Facts<'_>) -> mailrs_fraud::finding::Findings {
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("compiles");
    let out = rules.classify(f, &Policy::default());
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    out.findings
}

#[test]
fn a_parser_failure_scores_three_and_holds_nothing() {
    let f = Facts {
        from: "Accounts <ap@kawamura-shoji.co.jp>",
        domain: "kawamura-shoji.co.jp",
        registrable: "kawamura-shoji.co.jp",
        subject: "invoice",
        parser_failed_on_message: true,
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

/// Not in the corpus: a message with no `From:` at all. The rule does not
/// ask who sent it, so leaving the sender out does not step around it.
#[test]
fn a_message_without_a_sender_still_scores() {
    let f = Facts {
        parser_failed_on_message: true,
        ..Facts::default()
    };
    assert!(findings(&f).iter().any(|h| h.rule == RULE));
}

#[test]
fn a_message_read_in_full_is_not_touched() {
    let f = Facts {
        from: "Accounts <ap@kawamura-shoji.co.jp>",
        domain: "kawamura-shoji.co.jp",
        ..Facts::default()
    };
    assert!(!findings(&f).iter().any(|h| h.rule == RULE));
}
