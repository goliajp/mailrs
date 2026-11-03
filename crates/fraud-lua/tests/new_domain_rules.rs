//! The two rules on domains first seen this week: each fires only with its
//! second fact beside the new domain, never holds, and a domain that is
//! not new — or a store that could not say — fires neither.

use mailrs_fraud::novelty::Novelty;
use mailrs_fraud::payment::PaymentIds;
use mailrs_fraud::{Facts, Policy};
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

const PAYMENT: &str = "new-domain-asks-for-payment";
const LOGIN: &str = "new-domain-login-link";

fn base() -> Facts<'static> {
    Facts {
        from: "Accounts <ar@fresh-supplier.example>",
        domain: "mx.fresh-supplier.example",
        registrable: "fresh-supplier.example",
        subject: "invoice",
        domain_seen: 1,
        spf: "pass",
        dkim: "pass",
        dmarc: "pass",
        ..Facts::default()
    }
}

fn fired(facts: &Facts<'_>) -> Vec<(String, bool)> {
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("compiles");
    let out = rules.classify(facts, &Policy::default());
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    out.findings
        .iter()
        .filter(|f| f.rule == PAYMENT || f.rule == LOGIN)
        .map(|f| (f.rule.clone(), f.holds))
        .collect()
}

fn new_sender() -> Novelty {
    Novelty {
        sender_new: true,
        sender_age_days: Some(2),
        ..Novelty::default()
    }
}

fn iban() -> PaymentIds {
    PaymentIds {
        iban: true,
        iban_country: Some("NL"),
        crypto: false,
    }
}

#[test]
fn a_new_domain_with_a_payment_account_is_suspected_not_held() {
    let f = Facts {
        payment: iban(),
        novelty: new_sender(),
        ..base()
    };
    assert_eq!(fired(&f), [(PAYMENT.to_string(), false)]);
}

#[test]
fn a_new_domain_without_a_payment_account_fires_nothing() {
    let f = Facts {
        novelty: new_sender(),
        ..base()
    };
    assert!(fired(&f).is_empty());
}

#[test]
fn a_payment_account_from_a_known_domain_fires_nothing() {
    let f = Facts {
        payment: iban(),
        novelty: Novelty {
            sender_age_days: Some(400),
            ..Novelty::default()
        },
        ..base()
    };
    assert!(fired(&f).is_empty());
}

#[test]
fn a_new_link_domain_beside_a_misleading_link_or_a_login_form_is_suspected() {
    let novelty = Novelty {
        new_link_domain: Some("fresh-login.example".into()),
        ..Novelty::default()
    };
    let shown = Facts {
        shown_links: "bank.example sign-in.fresh-login.example\n",
        novelty: novelty.clone(),
        ..base()
    };
    assert_eq!(fired(&shown), [(LOGIN.to_string(), false)]);
    let mut form = Facts {
        novelty: novelty.clone(),
        ..base()
    };
    form.html.offsite_login_form = true;
    assert_eq!(fired(&form), [(LOGIN.to_string(), false)]);
}

#[test]
fn a_new_link_domain_alone_or_an_old_one_beside_a_misleading_link_fires_nothing() {
    let alone = Facts {
        novelty: Novelty {
            new_link_domain: Some("fresh-login.example".into()),
            ..Novelty::default()
        },
        ..base()
    };
    assert!(fired(&alone).is_empty());
    let old = Facts {
        shown_links: "bank.example sign-in.old.example\n",
        ..base()
    };
    assert!(fired(&old).is_empty());
}
