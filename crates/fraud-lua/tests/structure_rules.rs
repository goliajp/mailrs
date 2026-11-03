//! The HTML-attachment rules hold, the business-email-compromise rules
//! only score: what the attachment is built to do is the message's own
//! structure, while every BEC rule leans on the reader's history, and
//! familiarity convicts nobody.
use mailrs_fraud::html_attachment::HtmlFeatures;
use mailrs_fraud::payment::PaymentIds;
use mailrs_fraud::relation::Relation;
use mailrs_fraud::{Facts, Policy};
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

fn base() -> Facts<'static> {
    Facts {
        from: "Accounts <ap@kawamura-shoji.co.jp>",
        domain: "kawamura-shoji.co.jp",
        registrable: "kawamura-shoji.co.jp",
        subject: "invoice",
        domain_seen: 40,
        ..Facts::default()
    }
}

fn judge(f: &Facts<'_>) -> mailrs_fraud::finding::Findings {
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("compiles");
    let out = rules.classify(f, &Policy::default());
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    out.findings
}

#[test]
fn an_attachment_built_to_smuggle_a_file_is_held() {
    for (rule, html) in [
        (
            "html-builds-a-download",
            HtmlFeatures {
                builds_a_download: true,
                ..HtmlFeatures::default()
            },
        ),
        (
            "svg-runs-script",
            HtmlFeatures {
                svg_runs_script: true,
                ..HtmlFeatures::default()
            },
        ),
        (
            "attachment-is-a-login-page",
            HtmlFeatures {
                offsite_login_form: true,
                ..HtmlFeatures::default()
            },
        ),
    ] {
        let found = judge(&Facts { html, ..base() });
        assert!(found.has(rule), "{rule}: {:?}", found.rules());
        assert!(found.hold_worthy(), "{rule} holds");
    }
    let plain = judge(&base());
    assert!(!plain.hold_worthy(), "{:?}", plain.rules());
}

fn pays() -> PaymentIds {
    PaymentIds {
        iban: true,
        iban_country: Some("DE"),
        crypto: false,
    }
}

#[test]
fn each_bec_rule_fires_on_its_structure_and_never_holds() {
    let cases: [(&str, Facts<'static>); 5] = [
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
        ),
        (
            "bec-pay-to-a-lookalike",
            Facts {
                payment: pays(),
                relation: Relation {
                    looks_like_correspondent: true,
                    ..Relation::default()
                },
                ..base()
            },
        ),
        (
            "bec-name-borrowed",
            Facts {
                reply_to_outside: true,
                relation: Relation {
                    name_borrowed: true,
                    first_contact: true,
                    ..Relation::default()
                },
                ..base()
            },
        ),
        (
            "bec-lookalike-reply-diverted",
            Facts {
                reply_to_outside: true,
                relation: Relation {
                    looks_like_correspondent: true,
                    ..Relation::default()
                },
                ..base()
            },
        ),
        (
            "bec-reply-diverted-unauthenticated",
            Facts {
                reply_to_outside: true,
                dmarc: "fail",
                relation: Relation {
                    from_correspondent_domain: true,
                    ..Relation::default()
                },
                ..base()
            },
        ),
    ];
    for (rule, f) in &cases {
        let found = judge(f);
        assert!(found.has(rule), "{rule}: {:?}", found.rules());
        assert!(
            !found.hold_worthy(),
            "{rule} must not hold: {:?}",
            found.rules()
        );
    }
}

#[test]
fn half_a_pattern_is_nothing() {
    // a payment account from someone the reader knows
    assert!(
        judge(&Facts {
            payment: pays(),
            ..base()
        })
        .rules()
        .iter()
        .all(|r| !r.starts_with("bec-"))
    );
    // a first contact without a payment account
    let first = Facts {
        relation: Relation {
            first_contact: true,
            ..Relation::default()
        },
        ..base()
    };
    assert!(!judge(&first).has("bec-pay-to-a-stranger"));
    // DMARC not known yet on the first live pass
    let unknown = Facts {
        reply_to_outside: true,
        dmarc: "",
        relation: Relation {
            from_correspondent_domain: true,
            ..Relation::default()
        },
        ..base()
    };
    assert!(!judge(&unknown).has("bec-reply-diverted-unauthenticated"));
    // the correspondent's domain passing its own DMARC
    let passing = Facts {
        dmarc: "pass",
        ..unknown.clone()
    };
    assert!(!judge(&passing).has("bec-reply-diverted-unauthenticated"));
}
