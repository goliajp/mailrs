//! A `From:` at our own domain that nothing authenticated as us.
//!
//! Three conditions, and the version that shipped on 2026-09-18 had
//! one of them: "the session did not authenticate" held 62
//! conversations of this deployment's own system mail, because its
//! services submit to the MX without SMTP AUTH and some of them are
//! unsigned. The others — a public peer, and DMARC actually saying
//! `fail` — are what separate those from the BEC message this rule was
//! written for (`aiyhccspbu@golia.jp`, display name `齋藤 真`).
use mailrs_fraud::{Facts, Policy, RULE_CLAIMS_OUR_DOMAIN};
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

fn policy() -> Policy {
    Policy {
        our_domains: vec!["golia.jp".into(), "golia.ai".into()],
        allowed_domains: vec!["golia.atlassian.net".into()],
        ..Policy::default()
    }
}

/// One scan, with the three facts this rule turns on spelled out.
fn held(from: &str, unauthenticated: bool, peer_is_private: bool, dmarc: &str) -> bool {
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("the shipped bundle compiles");
    let facts = Facts {
        from,
        subject: "ギリア株式会社 業務変更",
        unauthenticated,
        peer_is_private,
        dmarc,
        ..Facts::default()
    };
    rules
        .classify(&facts, &policy())
        .findings
        .has(RULE_CLAIMS_OUR_DOMAIN)
}

const BEC: &str = "齋藤 真 <aiyhccspbu@golia.jp>";

#[test]
fn a_public_sender_at_our_domain_that_dmarc_failed_is_held() {
    assert!(held(BEC, true, false, "fail"));
    // Subdomains of ours are ours — `owns` matches on the suffix.
    assert!(held("齋藤 真 <x@mail.golia.jp>", true, false, "fail"));
}

/// This deployment's own services: `devops@golia.jp` and friends, from
/// the container bridge, `dmarc=fail` because nothing signed them.
/// Authentication cannot tell them from a forgery; the address they
/// came from can.
#[test]
fn our_own_systems_submitting_from_inside_are_not_held() {
    assert!(!held("devops@golia.jp", true, true, "fail"));
}

/// And mail from a public address that really is us — production has
/// `spf=pass dmarc=pass` senders at `golia.jp` — stays put.
#[test]
fn a_public_sender_that_dmarc_verified_is_not_held() {
    assert!(!held("noreply@golia.jp", true, false, "pass"));
}

/// Unknown must decline.  The receiver's first pass runs before the
/// stage that checks alignment, and the sweep may find a message with
/// no header to read; neither may guess.
#[test]
fn alignment_that_was_never_checked_declines() {
    for dmarc in ["", "none", "softfail", "temperror"] {
        assert!(!held(BEC, true, false, dmarc), "dmarc={dmarc:?}");
    }
}

/// An authenticated submission is us by definition — and that is not
/// this path anyway, since the receiver only scans sessions that did
/// not authenticate.
#[test]
fn an_authenticated_submission_is_not_held() {
    assert!(!held(BEC, false, false, "fail"));
}

#[test]
fn mail_from_elsewhere_is_left_to_the_other_rules() {
    assert!(!held("齋藤 真 <omqqy@wzglff.com>", true, false, "fail"));
}

#[test]
fn an_allowed_domain_is_exempt() {
    let mut rules = Rules::compile(DEFAULT_SOURCE).unwrap();
    let mut p = policy();
    p.our_domains.push("golia.atlassian.net".into());
    let facts = Facts {
        from: "LI HAO <jira@golia.atlassian.net>",
        unauthenticated: true,
        dmarc: "fail",
        ..Facts::default()
    };
    assert!(
        !rules
            .classify(&facts, &p)
            .findings
            .has(RULE_CLAIMS_OUR_DOMAIN)
    );
}
