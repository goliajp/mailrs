//! A sender nothing vouches for, whose every link is a cloud storage
//! bucket with a generated name. Runs in shadow.
use mailrs_fraud::{Facts, Policy};
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

const RULE: &str = "unvouched-sender-links-only-to-a-bucket";

fn policy() -> Policy {
    Policy {
        our_domains: vec!["golia.jp".into()],
        ..Policy::default()
    }
}

fn phish() -> Facts<'static> {
    Facts {
        from: "Card Services <info@k2v8.example>",
        subject: "ご利用確認",
        spf: "none",
        dkim: "fail",
        dmarc: "none",
        unauthenticated: true,
        has_links: true,
        links_only_to_random_buckets: true,
        bucket_hosts: "a8k3f9z2qq.s3.amazonaws.com",
        ..Facts::default()
    }
}

/// Whether the rule hit, and where it was recorded.
fn judge(facts: &Facts<'_>) -> (bool, bool) {
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("the shipped bundle compiles");
    let out = rules.classify(facts, &policy());
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    (out.shadow.has(RULE), out.findings.has(RULE))
}

#[test]
fn an_unvouched_sender_linking_only_to_a_bucket_is_recorded_in_shadow() {
    assert_eq!(judge(&phish()), (true, false));
}

#[test]
fn any_pass_vouches_for_the_sender() {
    for (spf, dkim, dmarc) in [
        ("pass", "fail", "none"),
        ("none", "pass", "none"),
        ("none", "fail", "pass"),
    ] {
        let f = Facts {
            spf,
            dkim,
            dmarc,
            ..phish()
        };
        assert_eq!(judge(&f), (false, false), "{spf} {dkim} {dmarc}");
    }
}

#[test]
fn it_needs_every_link_on_a_bucket() {
    let f = Facts {
        links_only_to_random_buckets: false,
        bucket_hosts: "",
        ..phish()
    };
    assert_eq!(judge(&f), (false, false));
}

#[test]
fn our_own_and_internal_senders_are_left_alone() {
    let ours = Facts {
        from: "IT <it@golia.jp>",
        ..phish()
    };
    assert_eq!(judge(&ours), (false, false));
    let inside = Facts {
        peer_is_private: true,
        ..phish()
    };
    assert_eq!(judge(&inside), (false, false));
    let submitted = Facts {
        unauthenticated: false,
        ..phish()
    };
    assert_eq!(judge(&submitted), (false, false));
}

/// The fact as the stored-message path reads it off a raw message, for
/// the rescan. A namespace declaration in the HTML is not a second link.
#[test]
fn the_stored_reading_finds_the_bucket_links() {
    let raw = b"From: Card <info@k2v8.example>\r\nSubject: x\r\nContent-Type: text/html\r\n\r\n\
<html xmlns=\"http://www.w3.org/1999/xhtml\"><a href=\"https://a8k3f9z2qq.s3.amazonaws.com/i.html\">login</a></html>\r\n";
    let reading = mailrs_fraud_lua::stored::Reading::of(raw, None, &[]);
    let f = reading.facts(0, 0, Default::default());
    assert!(f.links_only_to_random_buckets);
    assert_eq!(f.bucket_hosts, "a8k3f9z2qq.s3.amazonaws.com");

    let readable =
        b"From: Shop <info@shop.example>\r\nSubject: x\r\nContent-Type: text/html\r\n\r\n\
<a href=\"https://shop-newsletter-assets.s3.amazonaws.com/i.png\">x</a>\r\n";
    let reading = mailrs_fraud_lua::stored::Reading::of(readable, None, &[]);
    let f = reading.facts(0, 0, Default::default());
    assert!(!f.links_only_to_random_buckets);
    assert_eq!(f.bucket_hosts, "");
}
