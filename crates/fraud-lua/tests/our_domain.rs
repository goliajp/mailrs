//! A `From:` at our own domain, on a session that did not authenticate.
//!
//! The message that prompted these: `aiyhccspbu@golia.jp`, display name
//! `齋藤 真`, subject `ギリア株式会社 業務変更`, asking the reader to
//! reply with their personal LINE QR code.  It reached the inbox with a
//! "Suspicious sender" badge and no hold, because every hold-worthy
//! identity rule begins at `external(m)` — false for our own domains.
use mailrs_fraud::{Facts, Policy, RULE_CLAIMS_OUR_DOMAIN};
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

fn policy() -> Policy {
    Policy {
        our_domains: vec!["golia.jp".into(), "golia.ai".into()],
        allowed_domains: vec!["golia.atlassian.net".into()],
        ..Policy::default()
    }
}

fn scan(from: &str, unauthenticated: bool) -> mailrs_fraud::Findings {
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("the shipped bundle compiles");
    let facts = Facts {
        from,
        subject: "ギリア株式会社 業務変更",
        unauthenticated,
        ..Facts::default()
    };
    rules.classify(&facts, &policy()).findings
}

#[test]
fn an_unauthenticated_session_claiming_our_domain_is_held() {
    let f = scan("齋藤 真 <aiyhccspbu@golia.jp>", true);
    assert!(f.has(RULE_CLAIMS_OUR_DOMAIN), "{f:?}");
    assert!(f.hold_worthy(), "the finding did not hold: {f:?}");
}

/// Subdomains of ours are ours — `owns` matches on the suffix, and a
/// spoof is as likely to pick `mail.golia.jp` as the bare domain.
#[test]
fn a_subdomain_of_ours_counts_as_ours() {
    assert!(scan("齋藤 真 <x@mail.golia.jp>", true).has(RULE_CLAIMS_OUR_DOMAIN));
}

/// The sweep over stored mail cannot know how a message was submitted,
/// and mail our own people sent each other is at our domain by
/// definition.  Without the fact, the rule declines to fire — so a
/// rescan can never hold the mailbox's own internal history.
#[test]
fn without_the_fact_the_rule_declines() {
    let f = scan("齋藤 真 <aiyhccspbu@golia.jp>", false);
    assert!(!f.has(RULE_CLAIMS_OUR_DOMAIN), "{f:?}");
}

/// Ordinary outside mail is not this rule's business — the rules that
/// start at `external(m)` handle it, and this one must not double up.
#[test]
fn mail_from_elsewhere_is_not_this_rule() {
    assert!(!scan("齋藤 真 <omqqy@wzglff.com>", true).has(RULE_CLAIMS_OUR_DOMAIN));
}

/// A domain on the allow-list is one we told to send as us.
#[test]
fn an_allowed_domain_is_exempt() {
    let mut rules = Rules::compile(DEFAULT_SOURCE).unwrap();
    let mut p = policy();
    p.our_domains.push("golia.atlassian.net".into());
    let facts = Facts {
        from: "LI HAO <jira@golia.atlassian.net>",
        unauthenticated: true,
        ..Facts::default()
    };
    let f = rules.classify(&facts, &p).findings;
    assert!(!f.has(RULE_CLAIMS_OUR_DOMAIN), "{f:?}");
}
