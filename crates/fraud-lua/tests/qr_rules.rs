//! The QR rules run in shadow: their hits are recorded, nothing is scored
//! or held; each needs its own fact, and a stored message reaches them
//! through what extraction stored.

use kevy_client::Connection;
use mailrs_fraud::qr::QrFacts;
use mailrs_fraud::{Facts, Policy};
use mailrs_fraud_lua::stored::relation::Counted;
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

const TEXT: &str = "qr-text-says-another-domain";
const ONLY: &str = "qr-only-way-out";
const NEW: &str = "qr-to-a-new-domain";

fn base() -> Facts<'static> {
    Facts {
        from: "Billing <billing@supplier.example>",
        domain: "mx.supplier.example",
        registrable: "supplier.example",
        subject: "invoice",
        domain_seen: 40,
        spf: "pass",
        dkim: "pass",
        dmarc: "pass",
        ..Facts::default()
    }
}

fn policy() -> Policy {
    Policy {
        our_domains: vec!["golia.example".into()],
        ..Policy::default()
    }
}

/// `(shadow hits among the three, every enforced rule that fired)`.
fn run(facts: &Facts<'_>) -> (Vec<String>, Vec<String>) {
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("compiles");
    let out = rules.classify(facts, &policy());
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    let ours = [TEXT, ONLY, NEW];
    assert!(
        out.findings
            .iter()
            .all(|f| !ours.contains(&f.rule.as_str())),
        "a shadow rule scored: {:?}",
        out.findings.rules()
    );
    let mut shadow: Vec<String> = out
        .shadow
        .iter()
        .map(|f| f.rule.clone())
        .filter(|r| ours.contains(&r.as_str()))
        .collect();
    shadow.sort();
    (
        shadow,
        out.findings.rules().iter().map(|r| r.to_string()).collect(),
    )
}

fn code(host_not_in_message: bool, text_mismatch: bool) -> QrFacts {
    QrFacts {
        codes: 1,
        host_not_in_message,
        text_mismatch,
    }
}

#[test]
fn the_three_rules_are_shadow() {
    let rules = Rules::compile(DEFAULT_SOURCE).expect("compiles");
    for id in [TEXT, ONLY, NEW] {
        assert!(rules.shadow_rules().iter().any(|r| r == id), "{id}");
        assert!(!rules.enforced_rules().iter().any(|r| r == id), "{id}");
    }
}

#[test]
fn each_qr_rule_needs_its_own_fact() {
    let nothing = run(&base());
    assert!(nothing.0.is_empty(), "{nothing:?}");

    let printed_elsewhere = Facts {
        qr: code(true, true),
        qr_domains: "pay.example",
        has_links: true,
        ..base()
    };
    assert_eq!(run(&printed_elsewhere).0, [TEXT]);

    let only_a_code = Facts {
        qr: code(true, false),
        qr_domains: "pay.example",
        ..base()
    };
    assert_eq!(run(&only_a_code).0, [ONLY]);
    // with a link to follow, a code is not the only way out
    let with_links = Facts {
        has_links: true,
        ..only_a_code.clone()
    };
    assert!(run(&with_links).0.is_empty());
    // a code to the sender's own or a linked host is in the message
    let named = Facts {
        qr: code(false, false),
        ..only_a_code.clone()
    };
    assert!(run(&named).0.is_empty());

    let to_a_new_domain = Facts {
        new_qr_domain: Some("xn--pple-43d.example".into()),
        has_links: true,
        ..only_a_code
    };
    assert_eq!(run(&to_a_new_domain).0, [NEW]);
}

#[test]
fn a_shadow_hit_changes_no_score() {
    let all = Facts {
        qr: code(true, true),
        qr_domains: "pay.example",
        new_qr_domain: Some("pay.example".into()),
        ..base()
    };
    let (shadow, enforced) = run(&all);
    assert_eq!(shadow, [ONLY, TEXT, NEW]);
    assert_eq!(enforced, run(&base()).1);
}

#[test]
fn our_own_mail_fires_none() {
    let ours = Facts {
        from: "Desk <desk@golia.example>",
        domain: "mx.golia.example",
        registrable: "golia.example",
        qr: code(true, true),
        new_qr_domain: Some("pay.example".into()),
        ..base()
    };
    assert!(run(&ours).0.is_empty());
}

const ME: &str = "bob@golia.example";

/// A message from `from`, received by this deployment at `received`
/// (RFC 5322 date), whose text carries `body`.
fn raw(from: &str, received: &str, body: &str) -> String {
    format!(
        "Received: from mx.example by mx.golia.example; {received}\r\n\
         From: {from}\r\nTo: {ME}\r\nSubject: your order\r\n\
         Date: {received}\r\n\r\n{body}\r\n"
    )
}

/// The shadow rules a stored message hits, judged as `ME` reads it.
fn judged(
    raw: &str,
    texts: &[String],
    qr_urls: &[String],
    store: &mut Option<Connection>,
) -> Vec<String> {
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("compiles");
    let mut shadow = Vec::new();
    mailrs_fraud_lua::stored::judge_with(
        raw.as_bytes(),
        Some((ME, Counted::After)),
        None,
        mailrs_fraud_lua::stored::Extracted { texts, qr_urls },
        &policy(),
        store,
        &mut |facts, policy| {
            let out = rules.classify(facts, policy);
            assert!(out.errors.is_empty(), "{:?}", out.errors);
            shadow = out.shadow.iter().map(|f| f.rule.clone()).collect();
            Ok(mailrs_fraud_lua::Scan {
                findings: out.findings,
                version: String::new(),
                shadow: Vec::new(),
            })
        },
    )
    .expect("judged");
    shadow.retain(|r| [TEXT, ONLY, NEW].contains(&r.as_str()));
    shadow.sort();
    shadow
}

#[test]
fn a_stored_message_reads_its_qr_codes_from_what_extraction_stored() {
    let letter = raw(
        "Shop <info@shop.example>",
        "Mon, 2 Nov 2026 10:00:00 +0000",
        "Scan the code in the attached image to pay.",
    );
    let printed = vec!["Pay securely at www.shop.example".to_string()];
    let urls = vec!["https://pay.elsewhere.example/p?i=1".to_string()];
    assert_eq!(
        judged(&letter, &printed, &urls, &mut None),
        [ONLY, TEXT],
        "no store: the code's domain cannot be new"
    );
    // nothing stored: no codes, no hits; a core that reads no images
    assert!(judged(&letter, &printed, &[], &mut None).is_empty());
}
