//! A subject that is one word cut out of the recipient's address.
//!
//! Reported 2026-09-23: `Erica Flores <ericaf_flores@caredealspark.com>`,
//! subject `Hao`, to `lihao@golia.jp`, body `Hao, would 4 to 8 more
//! high-paying contracts ... move the needle? Let's chat.` Twenty-one
//! of these in the corpus, from twenty-one domains, and nothing else
//! with that subject shape.
use mailrs_fraud::{Facts, Policy};
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

const RULE: &str = "subject-cut-from-the-address";

fn policy() -> Policy {
    Policy {
        our_domains: vec!["golia.jp".into(), "golia.ai".into()],
        ..Policy::default()
    }
}

fn findings(from: &str, subject: &str, to_display: &str) -> mailrs_fraud::Findings {
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("the shipped bundle compiles");
    let facts = Facts {
        from,
        subject,
        to_display,
        ..Facts::default()
    };
    let out = rules.classify(&facts, &policy());
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    out.findings
}

fn fires(from: &str, subject: &str, to_display: &str) -> bool {
    findings(from, subject, to_display).has(RULE)
}

const REPORTED: &str = "Erica Flores <ericaf_flores@caredealspark.com>";

/// Both `To:` forms the campaign uses reach the rule as the address:
/// `"lihao@golia.jp" <lihao@golia.jp>` and the bare `lihao@golia.jp`.
#[test]
fn the_reported_message_is_caught() {
    assert!(fires(REPORTED, "Hao", "lihao@golia.jp"));
    assert!(fires(REPORTED, "Hao,", "lihao@golia.jp"));
}

/// Suspicion, not a verdict: it pushes to Junk and hides nothing.
#[test]
fn it_is_scored_and_reaches_the_junk_threshold_alone() {
    let f = findings(REPORTED, "Hao", "lihao@golia.jp");
    assert!(!f.hold_worthy());
    assert!(f.score() >= 5.0, "score {}", f.score());
}

/// A real name on the `To:` line leaves no address to cut from, and
/// the rule declines rather than guessing.
#[test]
fn a_to_line_with_a_real_name_declines() {
    assert!(!fires(REPORTED, "Hao", "Li Hao"));
    assert!(!fires(REPORTED, "Hao", ""));
}

/// The whole mailbox is not a piece of it, and a sentence is not a
/// name: `lihao` to `lihao@`, `Hao Li`, `Re: Hao`.
#[test]
fn only_a_single_word_that_is_part_of_the_mailbox_counts() {
    for subject in [
        "lihao", "Lihao", "Hao Li", "Re: Hao", "Hi", "H", "好", "Invoice",
    ] {
        assert!(!fires(REPORTED, subject, "lihao@golia.jp"), "{subject}");
    }
}

/// Our own people writing `Hao` to a colleague are not a mail merge.
#[test]
fn mail_from_our_own_domain_is_left_alone() {
    assert!(!fires("devops@golia.jp", "Hao", "lihao@golia.jp"));
}
