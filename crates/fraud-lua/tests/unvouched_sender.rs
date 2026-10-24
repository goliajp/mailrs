//! Two senders nothing can stand behind.
//!
//! Reported 2026-10-24: a tester's probes reached the inbox from
//! `probe@nodmarc-probe.invalid` and from a forged `ceo@gmail.com` with
//! `spf=softfail dkim=none dmarc=fail`.
use mailrs_fraud::{Facts, Policy};
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

const RESERVED: &str = "reserved-sender-domain";
const UNVOUCHED: &str = "nothing-vouches-for-the-sender";

fn policy() -> Policy {
    Policy {
        our_domains: vec!["golia.jp".into()],
        ..Policy::default()
    }
}

fn rules_for(from: &str, spf: &str, dkim: &str, dmarc: &str, peer_is_private: bool) -> Vec<String> {
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("the shipped bundle compiles");
    let facts = Facts {
        from,
        subject: "probe",
        spf,
        dkim,
        dmarc,
        peer_is_private,
        ..Facts::default()
    };
    let out = rules.classify(&facts, &policy());
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    out.findings.rules().into_iter().map(String::from).collect()
}

fn has(from: &str, spf: &str, dkim: &str, dmarc: &str, rule: &str) -> bool {
    rules_for(from, spf, dkim, dmarc, false)
        .iter()
        .any(|r| r == rule)
}

#[test]
fn reserved_domains_are_held() {
    assert!(has(
        "<probe@nodmarc-probe.invalid>",
        "none",
        "none",
        "none",
        RESERVED
    ));
    assert!(has("probe@example.org", "none", "none", "none", RESERVED));
    assert!(has("a@mail.example.com", "none", "none", "none", RESERVED));
    assert!(has("a@host.localhost", "none", "none", "none", RESERVED));
}

/// Names that only look like the reserved ones: not in the corpus, and
/// each a domain somebody can own.
#[test]
fn domains_that_merely_contain_a_reserved_word_are_not_held() {
    for from in [
        "a@myexample.com",
        "a@example.com.au",
        "a@invalid.co.jp",
        "a@contest.jp",
        "a@testing.io",
    ] {
        assert!(!has(from, "none", "none", "none", RESERVED), "{from}");
    }
}

/// Our own containers submit fixtures from the bridge.
#[test]
fn reserved_domains_from_inside_are_not_held() {
    let found = rules_for("probe@example.org", "none", "none", "none", true);
    assert!(!found.iter().any(|r| r == RESERVED), "{found:?}");
}

#[test]
fn a_forged_freemail_sender_is_held() {
    assert!(has(
        "<ceo@gmail.com>",
        "softfail",
        "none",
        "fail",
        UNVOUCHED
    ));
    assert!(has("x@investing.com", "fail", "none", "fail", UNVOUCHED));
    assert!(has("x@example.jp", "neutral", "none", "fail", UNVOUCHED));
}

/// A signature that failed to verify is a real sender's mail damaged on
/// the way, and a DNS outage is not a verdict.
#[test]
fn damaged_signatures_and_dns_errors_are_not_held() {
    assert!(!has("news@epark.jp", "neutral", "fail", "fail", UNVOUCHED));
    assert!(!has(
        "otp@focusai.com",
        "temperror",
        "none",
        "fail",
        UNVOUCHED
    ));
    assert!(!has(
        "otp@focusai.com",
        "permerror",
        "none",
        "fail",
        UNVOUCHED
    ));
}

/// No DMARC verdict means no claim to check; a domain without a record
/// is not convicted here.
#[test]
fn a_domain_without_dmarc_is_not_held() {
    assert!(!has(
        "a@small-shop.example.jp",
        "softfail",
        "none",
        "none",
        UNVOUCHED
    ));
    assert!(!has(
        "a@small-shop.example.jp",
        "softfail",
        "none",
        "",
        UNVOUCHED
    ));
}

#[test]
fn authenticated_senders_are_not_held() {
    assert!(!has("a@gmail.com", "pass", "none", "fail", UNVOUCHED));
    assert!(!has("a@gmail.com", "softfail", "pass", "fail", UNVOUCHED));
}

/// Our own domain is `claims-our-domain`'s question, not this rule's.
#[test]
fn our_own_domain_is_left_to_its_own_rule() {
    assert!(!has("ceo@golia.jp", "softfail", "none", "fail", UNVOUCHED));
}
