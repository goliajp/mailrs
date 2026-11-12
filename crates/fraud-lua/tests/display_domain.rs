//! `display-names-our-domain`: a display name that writes out one of
//! our own domains, on mail from someone else's.
use mailrs_fraud::relation::Relation;
use mailrs_fraud::{Facts, Policy};
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

const RULE: &str = "display-names-our-domain";

fn policy() -> Policy {
    Policy {
        our_domains: vec!["golia.jp".into(), "golia.ai".into()],
        allowed_domains: vec!["slack.com".into()],
        ..Policy::default()
    }
}

fn judge(facts: Facts<'_>, policy: &Policy) -> (bool, bool) {
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("the shipped bundle compiles");
    let out = rules.classify(&facts, policy);
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    let finding = out.findings.iter().find(|f| f.rule == RULE);
    (finding.is_some(), finding.is_some_and(|f| f.holds))
}

fn from(from: &str) -> Facts<'_> {
    Facts {
        from,
        subject: "Account setup requires a mandatory update today",
        spf: "pass",
        dmarc: "pass",
        ..Facts::default()
    }
}

/// The reported letter and the other six the corpus had, each held.
#[test]
fn our_domain_worn_by_a_stranger_is_held() {
    for f in [
        "Mail Delivery System - golia.jp<jade@stellarbbs.com>",
        "golia.jp I HR <admin@exportesram-ems.cam>",
        "golia.jp <sakaino@motorcoach.jp>",
        "\"FileRespond - golia.jp\" <abrian.kari@pge.id>",
        // not in the corpus: a subdomain, full-width letters, Japanese
        "IT 管理 mail.golia.ai <a@x.example>",
        "ＧＯＬＩＡ．ＪＰ システム管理者 <a@x.example>",
    ] {
        assert_eq!(judge(from(f), &policy()), (true, true), "{f}");
    }
}

/// The address in the display name of a sharing notice belongs to the
/// person who shared, and says nothing about the sender's domain.
#[test]
fn an_address_in_the_display_name_is_not_a_claim() {
    let f = from("\"lihao@golia.jp (Google Drive)\" <drive-shares-noreply@google.com>");
    assert_eq!(judge(f, &policy()), (false, false));
}

#[test]
fn our_own_mail_and_allowed_platforms_are_left_alone() {
    for f in [
        "golia.jp 管理者 <admin@golia.jp>",
        "golia.jp workspace <notify@slack.com>",
    ] {
        assert_eq!(judge(from(f), &policy()), (false, false), "{f}");
    }
}

#[test]
fn a_sender_the_reader_has_written_to_is_let_through() {
    let f = Facts {
        relation: Relation {
            from_correspondent: true,
            ..Relation::default()
        },
        ..from("golia.jp 担当 <sakaino@motorcoach.jp>")
    };
    assert_eq!(judge(f, &policy()), (false, false));
}

/// Another deployment's domains are its own claim, not ours; and with
/// no domains configured nothing fires.
#[test]
fn only_this_deployments_domains_count() {
    let f = "Mail Delivery System - example-corp.de <a@x.example>";
    assert_eq!(judge(from(f), &policy()), (false, false));
    let none = Policy::default();
    assert_eq!(
        judge(from("Mail Delivery System - golia.jp <a@x.example>"), &none),
        (false, false)
    );
}

/// A reader who let held mail from this address go has said it is not a
/// forgery; until they mark it junk, its next letter is not held either.
#[test]
fn a_sender_the_reader_released_is_let_through() {
    let f = Facts {
        relation: Relation {
            from_released: true,
            ..Relation::default()
        },
        ..from("golia.jp 担当 <sakaino@motorcoach.jp>")
    };
    assert_eq!(judge(f, &policy()), (false, false));
}
