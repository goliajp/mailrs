//! A platform's notification address carrying a user's text, with a link
//! through a shortener. Scored toward Junk, never held.
use mailrs_fraud::{Facts, Policy};
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

const RULE: &str = "platform-mail-links-through-a-shortener";

fn policy() -> Policy {
    Policy {
        our_domains: vec!["golia.jp".into()],
        ..Policy::default()
    }
}

fn hosts(list: &[&str]) -> Vec<String> {
    list.iter().map(|h| h.to_string()).collect()
}

fn appsheet(link_hosts: &[String]) -> Facts<'_> {
    Facts {
        from: "陳嘉欣 <noreply@appsheet.com>",
        subject: "希望確認受損產品處理方案",
        spf: "pass",
        dkim: "pass",
        dmarc: "pass",
        dkim_signed_by_from_org: true,
        has_links: !link_hosts.is_empty(),
        link_hosts,
        ..Facts::default()
    }
}

/// Whether the rule scored, and whether anything held.
fn judge(facts: &Facts<'_>) -> (bool, bool) {
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("the shipped bundle compiles");
    let out = rules.classify(facts, &policy());
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    (out.findings.has(RULE), out.findings.hold_worthy())
}

#[test]
fn the_reported_appsheet_letter_is_scored_and_not_held() {
    let h = hosts(&["share.google"]);
    assert_eq!(judge(&appsheet(&h)), (true, false));
}

#[test]
fn a_github_notification_with_a_short_link_is_scored() {
    let h = hosts(&["github.com", "bit.ly"]);
    let f = Facts {
        from: "Keydrejourney <notifications@github.com>",
        ..appsheet(&h)
    };
    assert!(judge(&f).0);
}

#[test]
fn a_person_sharing_a_short_link_from_their_own_mailbox_is_left_alone() {
    let h = hosts(&["share.google"]);
    let f = Facts {
        from: "陳嘉欣 <kayan.chan@gmail.com>",
        ..appsheet(&h)
    };
    assert!(!judge(&f).0);
}

#[test]
fn platform_mail_linking_only_to_the_platform_is_left_alone() {
    let h = hosts(&["www.appsheet.com", "github.com"]);
    assert!(!judge(&appsheet(&h)).0);
}

/// Not in the corpus: an address on the platform's domain that is not
/// its notification address, and a platform not on the list.
#[test]
fn only_the_listed_notification_addresses_count() {
    let h = hosts(&["share.google"]);
    for from in [
        "Support <support@appsheet.com>",
        "LinkedIn <notifications-noreply@linkedin.com>",
    ] {
        let f = Facts {
            from,
            ..appsheet(&h)
        };
        assert!(!judge(&f).0, "{from}");
    }
}

/// Not in the corpus: hosts that merely end in a shortener's name.
#[test]
fn a_host_is_a_shortener_only_by_its_own_domain() {
    let h = hosts(&["notshare.google", "at.co", "s.identity.example"]);
    assert!(!judge(&appsheet(&h)).0);
    let sub = hosts(&["go.bit.ly"]);
    assert!(judge(&appsheet(&sub)).0);
}

#[test]
fn an_unsigned_copy_of_the_address_is_left_to_the_other_rules() {
    let h = hosts(&["share.google"]);
    let f = Facts {
        dkim: "none",
        dkim_signed_by_from_org: false,
        ..appsheet(&h)
    };
    assert!(!judge(&f).0);
}

#[test]
fn a_sender_the_reader_wrote_to_is_left_alone() {
    let h = hosts(&["share.google"]);
    let mut f = appsheet(&h);
    f.relation.from_correspondent = true;
    assert!(!judge(&f).0);
}

/// The stored-message path reads the same hosts off a raw message.
#[test]
fn the_stored_reading_carries_every_link_host() {
    let raw = "From: =?UTF-8?B?6Zmz5ZiJ5qyj?= <noreply@appsheet.com>\r\nSubject: x\r\n\
Content-Type: text/plain; charset=utf-8\r\n\r\n\
\u{4f60}\u{597d}\r\nhttps://share.google/XqvT5LFKyII4EGOPY\r\n\r\nSent from AppSheet\r\n";
    let reading = mailrs_fraud_lua::stored::Reading::of(raw.as_bytes(), None, &[]);
    let f = reading.facts(0, 0, Default::default());
    assert_eq!(f.link_hosts, ["share.google".to_string()]);
}
