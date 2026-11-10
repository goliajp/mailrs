//! `burner-domain-outreach`: a first one-to-one message from a hosted
//! mailbox on a domain whose web root sends visitors elsewhere scores to
//! Junk; take away any one of those facts and it does not fire.
use mailrs_fraud::relation::Relation;
use mailrs_fraud::{Facts, Policy};
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

const RULE: &str = "burner-domain-outreach";

fn cold() -> Facts<'static> {
    Facts {
        from: "Emily Johnson <ejohnson@dealclosersvoice.com>",
        domain: "dealclosersvoice.com",
        registrable: "dealclosersvoice.com",
        subject: "Outreach for your ai and project management solutions",
        spf: "pass",
        dkim: "pass",
        dmarc: "pass",
        hosted_mailbox: true,
        site_elsewhere: true,
        relation: Relation {
            first_contact: true,
            ..Relation::default()
        },
        ..Facts::default()
    }
}

fn fires(facts: &Facts<'_>, policy: &Policy) -> Option<f64> {
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("shipped bundle compiles");
    let out = rules.classify(facts, policy);
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    out.findings
        .iter()
        .find(|f| f.rule == RULE)
        .map(|f| f.score)
}

#[test]
fn cold_outreach_from_a_burner_domain_scores_to_junk_without_holding() {
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("compiles");
    let out = rules.classify(&cold(), &Policy::default());
    let finding = out.findings.iter().find(|f| f.rule == RULE).expect("fires");
    assert!(
        finding.score >= 5.0,
        "reaches the Junk threshold on its own"
    );
    assert!(
        !mailrs_inbound::holds(&out.findings),
        "never hides the message"
    );
    assert_eq!(
        finding.params.get("domain").map(String::as_str),
        Some("dealclosersvoice.com")
    );
}

#[test]
fn each_fact_is_needed() {
    let p = Policy::default();
    assert_eq!(
        fires(
            &Facts {
                hosted_mailbox: false,
                ..cold()
            },
            &p
        ),
        None,
        "a mailing service"
    );
    assert_eq!(
        fires(
            &Facts {
                site_elsewhere: false,
                ..cold()
            },
            &p
        ),
        None,
        "a site that stays"
    );
    assert_eq!(
        fires(
            &Facts {
                is_bulk: true,
                ..cold()
            },
            &p
        ),
        None,
        "list mail"
    );
    let known = Relation {
        first_contact: false,
        ..Relation::default()
    };
    assert_eq!(
        fires(
            &Facts {
                relation: known,
                ..cold()
            },
            &p
        ),
        None,
        "not a first contact"
    );
}

/// Not in the corpus: a Japanese firm writing from a secondary domain to
/// a reader who has written to that domain before. The reader's own
/// history lets it through.
#[test]
fn a_domain_the_reader_wrote_to_is_let_through() {
    let wrote = Relation {
        first_contact: true,
        from_correspondent_domain: true,
        ..Relation::default()
    };
    let facts = Facts {
        from: "山田 太郎 <yamada@example-group.co.jp>",
        domain: "example-group.co.jp",
        registrable: "example-group.co.jp",
        relation: wrote,
        ..cold()
    };
    assert_eq!(fires(&facts, &Policy::default()), None);
}

#[test]
fn our_own_domains_are_never_outreach() {
    let policy = Policy {
        our_domains: vec!["dealclosersvoice.com".to_string()],
        ..Policy::default()
    };
    assert_eq!(fires(&cold(), &policy), None);
}
