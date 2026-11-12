//! The two rules on links that carry an address to a place the message
//! names nowhere else: held when the reader's address is encoded and
//! forwarded through another site, scored when any address sits in the
//! `#` part.
use mailrs_fraud::link_address::LinkAddress;
use mailrs_fraud::novelty::Novelty;
use mailrs_fraud::relation::Relation;
use mailrs_fraud::{Facts, Policy};
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

const FORWARDS: &str = "link-forwards-your-address";
const FRAGMENT: &str = "link-fragment-carries-an-address";

fn policy() -> Policy {
    Policy {
        our_domains: vec!["golia.jp".into()],
        ..Policy::default()
    }
}

/// (fired, held) for `rule`.
fn judge(rule: &str, facts: Facts<'_>) -> (bool, bool) {
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("the shipped bundle compiles");
    let out = rules.classify(&facts, &policy());
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    let f = out.findings.iter().find(|f| f.rule == rule);
    (f.is_some(), f.is_some_and(|f| f.holds))
}

fn forwarded(to: &str) -> Facts<'static> {
    Facts {
        from: "FileRespond <abrian.kari@pge.id>",
        link_address: LinkAddress {
            base64_via_redirect: Some(to.into()),
            ..LinkAddress::default()
        },
        ..Facts::default()
    }
}

fn fragment(to: &str) -> Facts<'static> {
    Facts {
        from: "New Document Shared <Golia.esign@lgmgsouthamerica.com>",
        link_address: LinkAddress {
            fragment_address: Some(to.into()),
            ..LinkAddress::default()
        },
        ..Facts::default()
    }
}

#[test]
fn an_encoded_address_forwarded_elsewhere_is_held() {
    assert_eq!(judge(FORWARDS, forwarded("smartechla.com")), (true, true));
}

#[test]
fn an_address_in_the_fragment_is_scored_not_held() {
    assert_eq!(judge(FRAGMENT, fragment("equltynj.com")), (true, false));
}

/// Every exemption, for both rules.
#[test]
fn known_senders_list_mail_and_our_own_places_are_left_alone() {
    for (rule, make) in [
        (FORWARDS, forwarded as fn(&str) -> Facts<'static>),
        (FRAGMENT, fragment),
    ] {
        let ours = Facts {
            from: "Notice <noreply@golia.jp>",
            ..make("x.example")
        };
        let bulk = Facts {
            is_bulk: true,
            ..make("x.example")
        };
        let wrote = Facts {
            relation: Relation {
                from_correspondent: true,
                ..Relation::default()
            },
            ..make("x.example")
        };
        let released = Facts {
            relation: Relation {
                from_released: true,
                ..Relation::default()
            },
            ..make("x.example")
        };
        let established = Facts {
            novelty: Novelty {
                sender_seen: 5,
                ..Novelty::default()
            },
            ..make("x.example")
        };
        let to_us = make("golia.jp");
        for (what, f) in [
            ("our own sender", ours),
            ("list mail", bulk),
            ("a correspondent", wrote),
            ("a released sender", released),
            ("an established sender domain", established),
            ("a destination of ours", to_us),
        ] {
            assert_eq!(judge(rule, f), (false, false), "{rule}: {what}");
        }
    }
}

/// The genuine shape the corpus does not have, kept for the 2026-12-11
/// review: a new service's verification mail with the reader's address
/// in the `#` part, straight to its app on a domain the message names
/// nowhere else. As the rules stand it is scored (Junk), not held.
#[test]
fn a_new_services_verification_mail_is_scored_not_held() {
    let f = Facts {
        from: "NewService <no-reply@mail.newservice.io>",
        link_address: LinkAddress {
            fragment_address: Some("newservice.example".into()),
            ..LinkAddress::default()
        },
        ..Facts::default()
    };
    assert_eq!(judge(FRAGMENT, f), (true, false));
}
