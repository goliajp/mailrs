//! A letter that offers its reader a cut of millions.
//!
//! Reported 2026-09-28: `David Konczol <info@nexforce.in>`, `RE:
//! PARTNERSHIP PROPOSITION`, an unclaimed US$10,200,000 policy to be
//! split 50% apiece — delivered to the inbox because it came from a
//! stolen account that passed every authentication check.
use mailrs_fraud::{Facts, Policy};
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

const RULE: &str = "offers-the-reader-a-sum";

fn findings(offers: bool, bulk: bool) -> mailrs_fraud::Findings {
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("the shipped bundle compiles");
    let facts = Facts {
        from: "David Konczol <info@nexforce.in>",
        subject: "RE: PARTNERSHIP PROPOSITION",
        spf: "pass",
        dkim: "pass",
        dmarc: "pass",
        offers_the_reader_a_sum: offers,
        is_bulk: bulk,
        ..Facts::default()
    };
    let out = rules.classify(&facts, &Policy::default());
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    out.findings
}

/// Suspicion, not a verdict: into Junk on its own, hidden never.
#[test]
fn the_reported_letter_reaches_junk_and_is_not_held() {
    let f = findings(true, false);
    assert!(f.has(RULE));
    assert!(!f.hold_worthy());
    assert!(f.score() >= 5.0, "score {}", f.score());
}

#[test]
fn a_mailing_list_is_excused() {
    assert!(!findings(true, true).has(RULE));
}

#[test]
fn no_offer_no_finding() {
    assert!(!findings(false, false).has(RULE));
}
