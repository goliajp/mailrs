//! A rule can name the facts a reader needs — the brand worn, the host a
//! link really goes to — as params beside its English detail, and each
//! client words them in the reader's language. A rule that returns only a
//! string, as every bundle before this did, works as it always has.
use std::collections::{BTreeMap, BTreeSet};

use mailrs_fraud::payment::PaymentIds;
use mailrs_fraud::relation::Relation;
use mailrs_fraud::{Facts, Policy};
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

fn run(source: &str, facts: &Facts<'_>) -> mailrs_fraud_lua::Evaluation {
    let mut rules = Rules::compile(source).expect("compiles");
    rules.classify(facts, &Policy::default())
}

#[test]
fn a_rule_that_returns_a_string_still_finds() {
    let out = run(
        "rule('old-style', 'content', 1, false, function(m) return 'saw it' end)",
        &Facts::default(),
    );
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    let f = out.findings.iter().next().expect("a finding");
    assert_eq!(
        (f.rule.as_str(), f.detail.as_str()),
        ("old-style", "saw it")
    );
    assert!(f.params.is_empty());
}

#[test]
fn a_rule_can_name_params_beside_its_detail() {
    let out = run(
        "rule('new-style', 'content', 1, false, function(m)
            return {detail = 'saw it', params = {host = 'a.example', shown_as = 'b.example'}}
        end)",
        &Facts::default(),
    );
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    let f = out.findings.iter().next().expect("a finding");
    assert_eq!(f.detail, "saw it");
    assert_eq!(
        f.params,
        BTreeMap::from([
            ("host".to_string(), "a.example".to_string()),
            ("shown_as".to_string(), "b.example".to_string()),
        ])
    );
}

#[test]
fn params_out_of_shape_are_the_rules_error_not_a_finding() {
    for params in [
        "{Host = 'x'}",
        "{['host name'] = 'x'}",
        "{host = 3}",
        &format!("{{host = '{}'}}", "x".repeat(257)),
        "{a='1',b='1',c='1',d='1',e='1',f='1',g='1',h='1',i='1'}",
        "'not a table'",
    ] {
        let source = format!(
            "rule('bad', 'content', 1, false, function(m) return {{detail = 'd', params = {params}}} end)"
        );
        let out = run(&source, &Facts::default());
        assert!(
            out.findings.is_empty(),
            "{params}: {:?}",
            out.findings.rules()
        );
        assert_eq!(out.errors.len(), 1, "{params}");
    }
}

fn base() -> Facts<'static> {
    Facts {
        from: "Someone <someone@sender.example>",
        domain: "mx.sender.example",
        registrable: "sender.example",
        subject: "hello",
        domain_seen: 40,
        spf: "pass",
        dkim: "pass",
        dmarc: "pass",
        ..Facts::default()
    }
}

fn policy() -> Policy {
    Policy {
        org_names: vec!["GOLIA株式会社".into()],
        our_domains: vec!["golia.jp".into()],
        account_names: vec!["Li Hao".into()],
        ..Policy::default()
    }
}

fn pays() -> PaymentIds {
    PaymentIds {
        iban: true,
        iban_country: Some("DE"),
        crypto: false,
    }
}

fn lookalike() -> Relation {
    Relation {
        looks_like_correspondent: true,
        lookalike_of: Some("kawamura-shoji.co.jp".into()),
        ..Relation::default()
    }
}

/// One message per rule that names facts, and the params it should carry.
/// A rule, a message it fires on, and the params it should carry.
type Case = (
    &'static str,
    Facts<'static>,
    &'static [(&'static str, &'static str)],
);

fn cases() -> Vec<Case> {
    vec![
        (
            "claims-our-name",
            Facts {
                from: "GOLIA株式会社 <info@sender.example>",
                ..base()
            },
            &[("name", "GOLIA株式会社")],
        ),
        (
            "impersonates-one-of-us",
            Facts {
                from: "Li Hao <ceo@sender.example>",
                ..base()
            },
            &[("name", "Li Hao"), ("domain", "sender.example")],
        ),
        (
            "subject-is-our-name",
            Facts {
                subject: "【GOLIA株式会社】",
                ..base()
            },
            &[("name", "golia株式会社"), ("domain", "sender.example")],
        ),
        (
            "brand-is-the-display-name",
            Facts {
                from: "PayPal <x@sender.example>",
                ..base()
            },
            &[("brand", "paypal"), ("domain", "sender.example")],
        ),
        (
            "impersonates-brand",
            Facts {
                from: "PayPal Support <x@sender.example>",
                domain_seen: 0,
                ..base()
            },
            &[("brand", "paypal"), ("domain", "sender.example")],
        ),
        (
            "subject-claims-brand",
            Facts {
                subject: "【JCB】ご利用確認",
                domain_seen: 0,
                ..base()
            },
            &[("brand", "【JCB"), ("domain", "mx.sender.example")],
        ),
        (
            "hostname-claims-company",
            Facts {
                domain: "paypal.mailer-host.com",
                ..base()
            },
            &[("company", "paypal"), ("domain", "mailer-host.com")],
        ),
        (
            "claims-our-domain",
            Facts {
                from: "IT <it@golia.jp>",
                unauthenticated: true,
                dmarc: "fail",
                ..base()
            },
            &[("domain", "golia.jp")],
        ),
        (
            "reserved-sender-domain",
            Facts {
                from: "probe <p@probe.invalid>",
                ..base()
            },
            &[("domain", "probe.invalid")],
        ),
        (
            "nothing-vouches-for-the-sender",
            Facts {
                dmarc: "fail",
                dkim: "none",
                spf: "softfail",
                ..base()
            },
            &[("domain", "sender.example"), ("spf", "softfail")],
        ),
        (
            "minted-address",
            Facts {
                from: "x <qwrtzk@bcdfghk.com>",
                ..base()
            },
            &[("address", "qwrtzk@bcdfghk.com")],
        ),
        (
            "greets-a-stranger",
            Facts {
                subject: "王伟，你好",
                to_display: "lihao@golia.jp",
                ..base()
            },
            &[("name", "王伟")],
        ),
        (
            "subject-cut-from-the-address",
            Facts {
                subject: "Hao",
                to_display: "lihao@golia.jp",
                ..base()
            },
            &[("word", "Hao")],
        ),
        (
            "subject-names-the-recipient",
            Facts {
                subject: "Password Reminder for lihao@golia.jp",
                to_display: "lihao@golia.jp",
                ..base()
            },
            &[("address", "lihao@golia.jp")],
        ),
        (
            "x-mailer-generated",
            Facts {
                x_mailer: Some("Bluewave Courier 3.1"),
                ..base()
            },
            &[("mailer", "Bluewave Courier 3.1")],
        ),
        (
            "reply-domain-rotation",
            Facts {
                reply_rotation: 5,
                ..base()
            },
            &[("count", "5")],
        ),
        (
            "minted-sending-host",
            Facts {
                domain: "x7kq9zt.mailhost.com",
                ..base()
            },
            &[("host", "x7kq9zt")],
        ),
        (
            "dated-in-the-future",
            Facts {
                date_ahead: 3 * 86_400 + 5,
                ..base()
            },
            &[("days", "3")],
        ),
        (
            "bec-pay-to-a-stranger",
            Facts {
                payment: pays(),
                relation: Relation {
                    first_contact: true,
                    ..Relation::default()
                },
                ..base()
            },
            &[("account", "IBAN (DE)"), ("domain", "sender.example")],
        ),
        (
            "bec-pay-to-a-lookalike",
            Facts {
                payment: pays(),
                relation: lookalike(),
                ..base()
            },
            &[
                ("account", "IBAN (DE)"),
                ("domain", "sender.example"),
                ("like", "kawamura-shoji.co.jp"),
            ],
        ),
        (
            "bec-name-borrowed",
            Facts {
                payment: pays(),
                relation: Relation {
                    name_borrowed: true,
                    ..Relation::default()
                },
                ..base()
            },
            &[("domain", "sender.example")],
        ),
        (
            "bec-lookalike-reply-diverted",
            Facts {
                relation: lookalike(),
                reply_to_outside: true,
                ..base()
            },
            &[
                ("domain", "sender.example"),
                ("like", "kawamura-shoji.co.jp"),
            ],
        ),
        (
            "bec-reply-diverted-unauthenticated",
            Facts {
                relation: Relation {
                    from_correspondent_domain: true,
                    ..Relation::default()
                },
                reply_to_outside: true,
                dmarc: "fail",
                ..base()
            },
            &[("domain", "sender.example")],
        ),
        (
            "link-shows-our-address",
            Facts {
                shown_links: "golia.jp login.evil.example\n",
                ..base()
            },
            &[("shown", "golia.jp"), ("target", "login.evil.example")],
        ),
    ]
}

/// The placeholders a sentence names, sorted.
fn placeholders(s: &str) -> BTreeSet<String> {
    s.split('{')
        .skip(1)
        .filter_map(|p| p.split_once('}').map(|(n, _)| n.to_string()))
        .collect()
}

#[test]
fn each_shipped_rule_names_the_facts_its_sentence_needs() {
    let table: serde_json::Value =
        serde_json::from_str(include_str!("../../inbound/src/reasons.json")).expect("reasons.json");
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("compiles");
    let mut covered = BTreeSet::new();
    for (rule, facts, want) in cases() {
        let out = rules.classify(&facts, &policy());
        assert!(out.errors.is_empty(), "{rule}: {:?}", out.errors);
        let f = out
            .findings
            .iter()
            .find(|f| f.rule == rule)
            .unwrap_or_else(|| panic!("{rule} did not fire: {:?}", out.findings.rules()));
        let want: BTreeMap<String, String> = want
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        assert_eq!(f.params, want, "{rule}");
        let sentence = table[rule]["params"]["en"]
            .as_str()
            .unwrap_or_else(|| panic!("{rule} has no sentence with params"));
        assert_eq!(
            placeholders(sentence),
            want.keys().cloned().collect::<BTreeSet<_>>(),
            "{rule}: the sentence names other params than the rule sets"
        );
        covered.insert(rule.to_string());
    }
    // every sentence with params belongs to a rule shown setting them here
    let with_params: BTreeSet<String> = table
        .as_object()
        .expect("object")
        .iter()
        .filter(|(_, v)| v.get("params").is_some())
        .map(|(k, _)| k.clone())
        .collect();
    let unshown: Vec<_> = with_params.difference(&covered).collect();
    assert!(
        unshown.is_empty(),
        "sentences with params no case covers: {unshown:?}"
    );
}
