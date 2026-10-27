//! Links that show our address and go elsewhere, a kit's unfilled
//! template in a link, and a subject naming the recipient's address.
//!
//! Reported 2026-10-27: `IT Security Officer <doc@wbg-ae.com>`, subject
//! `Password Reminder for lihao@golia.jp`, SPF and DKIM passing, with the
//! text `www. golia.jp /lihao@golia.jp/check-activity/` linking to
//! `webmail-oauth.wzldq.com/vrill/auth[[random_digits(8)]]/…`.
use mailrs_fraud::{Facts, Findings, Policy};
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

const SHOWS: &str = "link-shows-our-address";
const KIT: &str = "link-kit-placeholder";
const SUBJECT: &str = "subject-names-the-recipient";
const REPORTED_FROM: &str = "IT Security Officer <doc@wbg-ae.com>";

fn policy() -> Policy {
    Policy {
        our_domains: vec!["golia.jp".into(), "golia.ai".into()],
        ..Policy::default()
    }
}

fn classify(facts: &Facts<'_>, policy: &Policy) -> Findings {
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("the shipped bundle compiles");
    let out = rules.classify(facts, policy);
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    out.findings
}

/// The reported message, end to end: its HTML through the extractor the
/// receiver and the sweep both use, then the shipped rules.
#[test]
fn the_reported_message_is_held_for_review() {
    let raw = "From: IT Security Officer <doc@wbg-ae.com>\r\n\
To: \"lihao@golia.jp\" <lihao@golia.jp>\r\n\
Subject: Password Reminder for lihao@golia.jp\r\n\
MIME-Version: 1.0\r\n\
Content-Type: text/html; charset=us-ascii\r\n\r\n\
<p>Your password for lihao@golia.jp is about to expire soon.</p>\r\n\
You may visit&nbsp;<a style=\"color: rgb(17, 85, 204);\" \
href=\"https://webmail-oauth.wzldq.com/vrill/auth[[random_digits(8)]]/cpses[[random_string(10)]]#bGloYW9AZ29saWEuanA=\" \
target=\"_blank\">\r\nwww. golia.jp /lihao@golia.jp/check-activity/</a>&nbsp;to see email activity\r\n";
    let parsed = mailrs_mime::parse(raw.as_bytes());
    let shown = mailrs_inbound::body::shown_links(&parsed);
    assert_eq!(shown, "www.golia.jp webmail-oauth.wzldq.com");
    let facts = Facts {
        from: REPORTED_FROM,
        subject: "Password Reminder for lihao@golia.jp",
        to_display: "lihao@golia.jp",
        shown_links: &shown,
        link_has_kit_placeholder: mailrs_inbound::body::link_has_kit_placeholder(&parsed),
        spf: "pass",
        dkim: "pass",
        dmarc: "none",
        ..Facts::default()
    };
    let f = classify(&facts, &policy());
    assert!(f.has(SHOWS) && f.has(KIT) && f.has(SUBJECT), "{f:?}");
    assert!(f.hold_worthy());
}

#[test]
fn a_link_that_shows_our_address_holds_only_when_it_goes_elsewhere() {
    let fires = |shown_links: &str| {
        classify(
            &Facts {
                from: REPORTED_FROM,
                shown_links,
                ..Facts::default()
            },
            &policy(),
        )
        .has(SHOWS)
    };
    assert!(fires("www.golia.jp evil.example"));
    assert!(fires("example.org tracker.example\ngolia.ai evil.example"));
    // ours to ours: our own site behind another of our hosts
    assert!(!fires("www.golia.jp links.golia.jp"));
    assert!(!fires("golia.jp golia.ai"));
    // somebody else's address shown over a tracker: newsletters do this
    assert!(!fires("example.org tracker.example"));
    // not in the corpus: a host that only begins with our domain is not ours
    assert!(!fires("golia.jp.evil.example evil.example"));
    assert!(!fires(""));
}

#[test]
fn our_own_mail_and_an_empty_domain_list_convict_nobody() {
    let facts = Facts {
        from: "News <news@golia.jp>",
        shown_links: "www.golia.jp tracker.example",
        ..Facts::default()
    };
    assert!(!classify(&facts, &policy()).has(SHOWS));
    let facts = Facts {
        from: REPORTED_FROM,
        shown_links: "www.golia.jp evil.example",
        ..Facts::default()
    };
    assert!(!classify(&facts, &Policy::default()).has(SHOWS));
}

#[test]
fn the_kit_placeholder_and_the_subject_are_scored_not_held() {
    let kit = classify(
        &Facts {
            from: REPORTED_FROM,
            link_has_kit_placeholder: true,
            ..Facts::default()
        },
        &policy(),
    );
    assert!(
        kit.has(KIT) && !kit.hold_worthy() && kit.score() >= 5.0,
        "{kit:?}"
    );

    let subject = classify(
        &Facts {
            from: "golia.jp <sakaino@motorcoach.jp>",
            subject: "Support Service: LiHao@Golia.jp Password Expiry Alert!",
            to_display: "lihao@golia.jp",
            ..Facts::default()
        },
        &policy(),
    );
    assert!(
        subject.has(SUBJECT) && !subject.hold_worthy() && subject.score() >= 5.0,
        "{subject:?}"
    );
}

#[test]
fn the_subject_rule_needs_the_whole_address_from_the_to_line() {
    let fires = |subject: &str, to_display: &str| {
        classify(
            &Facts {
                from: REPORTED_FROM,
                subject,
                to_display,
                ..Facts::default()
            },
            &policy(),
        )
        .has(SUBJECT)
    };
    assert!(fires("Attention! takagi@golia.jp", "takagi@golia.jp"));
    // a real name on the `To:` line leaves no address to compare
    assert!(!fires("Password Reminder for lihao@golia.jp", "Li Hao"));
    // a piece of the address is another rule's business
    assert!(!fires("Hello lihao", "lihao@golia.jp"));
    // not in the corpus: somebody else's address in the subject
    assert!(!fires("Invoice for sales@golia.jp", "lihao@golia.jp"));
    // not in the corpus: a non-Latin subject and display name
    assert!(!fires("李好様 パスワードの有効期限", "李好"));
}
