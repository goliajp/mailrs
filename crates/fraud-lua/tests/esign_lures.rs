//! Fake signature requests: the rules that turn on the link carrying the
//! reader's disguised address, a display name in look-alike letters, and
//! a display name that is an e-signature service's name from someone
//! else's domain.
use mailrs_fraud::relation::Relation;
use mailrs_fraud::{Facts, Policy};
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

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

fn from(from: &str) -> Facts<'_> {
    Facts {
        from,
        spf: "pass",
        dkim: "pass",
        dmarc: "pass",
        ..Facts::default()
    }
}

const DISGUISED: &str = "link-disguises-the-recipient";

#[test]
fn a_disguised_address_in_a_link_is_held() {
    let f = Facts {
        link_disguises_recipient: true,
        ..from("Document Signature Request <billy@jenkinsguidingservice.com>")
    };
    assert_eq!(judge(DISGUISED, f), (true, true));
}

#[test]
fn our_own_mail_and_released_senders_are_left_alone() {
    let ours = Facts {
        link_disguises_recipient: true,
        ..from("Notice <noreply@golia.jp>")
    };
    assert_eq!(judge(DISGUISED, ours), (false, false));
    let released = Facts {
        link_disguises_recipient: true,
        relation: Relation {
            from_released: true,
            ..Relation::default()
        },
        ..from("Document Signature Request <billy@jenkinsguidingservice.com>")
    };
    assert_eq!(judge(DISGUISED, released), (false, false));
}

const MIXED: &str = "display-mixes-scripts";

#[test]
fn look_alike_letters_in_the_display_name_are_scored_not_held() {
    assert_eq!(
        judge(
            MIXED,
            from("ᏚhiеldеdFilе - ꓖoliа <jschuster@schustermanagement.com>")
        ),
        (true, false)
    );
    // not in the corpus: a Russian name beside a Latin one, Greek alone
    for name in ["Ivan Петров <ivan@x.example>", "Γιώργος <g@x.example>"] {
        assert_eq!(judge(MIXED, from(name)), (false, false), "{name}");
    }
}

const BRAND: &str = "brand-is-the-display-name";

#[test]
fn an_esignature_service_name_from_another_domain_is_held() {
    for f in [
        "DocuSign <dse.eumail-docusign@xjgh.org>",
        "Adobe Sign <echosign@sign-docs.example>",
        "Dropbox Sign <noreply@x.example>",
        "PandaDoc <docs@x.example>",
    ] {
        assert_eq!(judge(BRAND, from(f)), (true, true), "{f}");
    }
}

#[test]
fn the_services_own_mail_is_left_alone() {
    for f in [
        "Qualcomm Esignature via Docusign <dse_na2@docusign.net>",
        "DocuSign <dse@docusign.net>",
        "Docusign <dse@eumail.docusign.com>",
        "Adobe Acrobat Sign <adobesign@adobesign.com>",
        "Adobe Sign <echosign@echosign.com>",
        "Dropbox Sign <noreply@mail.hellosign.com>",
        "PandaDoc <docs@email.pandadoc.net>",
    ] {
        assert_eq!(judge(BRAND, from(f)), (false, false), "{f}");
    }
}
