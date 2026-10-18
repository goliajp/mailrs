//! Text from PDF attachments reaches the rules, and the shipped rules
//! do not read it yet.
use mailrs_fraud::{Facts, Policy};
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

const OFFER: &str = "in the sum of US$10,200,000.00, split equally, 50% apiece";

fn facts(attachment_text: &str, offers: bool) -> Facts<'_> {
    Facts {
        from: "Konczol <info@nexforce.example>",
        subject: "contract attached",
        attachment_text,
        attachment_offers_the_reader_a_sum: offers,
        ..Facts::default()
    }
}

#[test]
fn a_rule_can_read_both_attachment_facts() {
    let source = format!(
        "{DEFAULT_SOURCE}\nrule('probe-attachment', 'content', 1, false, function(m)\n\
         if m.attachment_offers_the_reader_a_sum and string.find(m.attachment_text, 'apiece', 1, true) then\n\
         return 'seen' end\nend)\n"
    );
    let mut rules = Rules::compile(&source).expect("compiles");
    let seen = rules.classify(&facts(OFFER, true), &Policy::default());
    assert!(seen.errors.is_empty(), "{:?}", seen.errors);
    assert!(seen.findings.has("probe-attachment"));
    let unseen = rules.classify(&facts("", false), &Policy::default());
    assert!(!unseen.findings.has("probe-attachment"));
}

#[test]
fn the_shipped_rules_judge_the_same_with_or_without_attachment_text() {
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("compiles");
    let without = rules.classify(&facts("", false), &Policy::default());
    let with = rules.classify(&facts(OFFER, true), &Policy::default());
    assert!(with.errors.is_empty(), "{:?}", with.errors);
    assert_eq!(with.findings.rules(), without.findings.rules());
    assert_eq!(with.findings.score(), without.findings.score());
}

#[test]
fn attachment_text_at_the_cap_is_accepted() {
    let text = "a".repeat(mailrs_fraud::ATTACHMENT_TEXT_CAP);
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("compiles");
    let out = rules.classify(&facts(&text, false), &Policy::default());
    assert!(out.errors.is_empty(), "{:?}", out.errors);
}
