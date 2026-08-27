//! The scam that authenticates correctly.
//!
//! A From header reading `GOLIA株式会社 <ipdxuawesj@auto360d.com>`, or
//! the same with an invented colleague's name, asking the reader to
//! send their LINE QR code so "work contact" can move there. Twenty-five
//! arrived in a 35,799-message corpus and **every one passed SPF**; 18
//! passed DKIM and DMARC too, because the sender owns the throwaway
//! domain and configures it properly.
//!
//! So the sender-trust path cannot see them, and this is the test that
//! says the other path can. Header block is one of the real ones, with
//! the display name encoded the way it actually arrived.

use mailrs_inbound::impersonation::claims_our_name;
use mailrs_inbound::{DeliveryDecision, from_header, make_delivery_decision};

fn names() -> Vec<String> {
    vec!["GOLIA株式会社".into()]
}
fn ours() -> Vec<String> {
    vec!["golia.jp".into(), "golia.ai".into()]
}
fn allowed() -> Vec<String> {
    vec!["slack.com".into(), "github.com".into()]
}

/// The From line as it arrives — base64 inside an encoded-word, which
/// is how a check on the raw header sees only ASCII and answers no.
fn scam_message() -> Vec<u8> {
    let encoded = mailrs_rfc2047::encode("GOLIA株式会社");
    format!(
        "Return-Path: <ipdxuawesj@auto360d.com>\r\n\
         Authentication-Results: mail.golia.ai; spf=pass; dkim=pass; dmarc=pass\r\n\
         From: {encoded} <ipdxuawesj@auto360d.com>\r\n\
         Subject: =?UTF-8?B?5qWt5YuZ6YCj57Wh44Gr44Gk44GE44Gm?=\r\n\
         \r\n\
         LINEのQRコードを添付していただけますでしょうか。\r\n"
    )
    .into_bytes()
}

#[test]
fn the_encoded_display_name_is_read_and_caught() {
    let from = from_header(&scam_message());
    assert!(
        from.contains("GOLIA株式会社"),
        "the encoded-word was not decoded: {from}"
    );
    assert!(claims_our_name(&from, &names(), &ours(), &allowed()));
}

/// It reaches Junk with the content signal such a message carries, and
/// the reason says which check did it.
#[test]
fn it_reaches_junk_and_says_why() {
    let mut input = mailrs_inbound::PipelineInput {
        greylisted: false,
        auth: mailrs_inbound::AuthResults {
            spf: "pass".into(),
            dkim: "pass".into(),
            arc: "none".into(),
            dmarc: "pass".into(),
            dmarc_policy: mailrs_inbound::DmarcPolicy::Pass,
        },
        virus_found: None,
        content_score: 1.0,
        matched_rules: vec![],
        ptr_score: 0.0,
        ai_score: 0.0,
        deception: Default::default(),
        claims_our_name: false,
        spam_threshold: 5.0,
        hostname: "mx.golia.ai".into(),
        from_addr: "ipdxuawesj@auto360d.com".into(),
        recipient_whitelist: std::collections::HashSet::new(),
        recipient_blacklist: std::collections::HashSet::new(),
        local_domains: std::collections::HashSet::new(),
    };

    // Authentication alone lets it straight through — this is the part
    // the existing defences get wrong, and it has to be shown.
    assert!(
        matches!(
            make_delivery_decision(&input),
            DeliveryDecision::Accept { .. }
        ),
        "a passing SPF/DKIM/DMARC scam was already being caught, which it was not"
    );

    input.claims_our_name =
        claims_our_name(&from_header(&scam_message()), &names(), &ours(), &allowed());
    let DeliveryDecision::Junk { reason, .. } = make_delivery_decision(&input) else {
        panic!("the impersonation signal did not carry it to Junk");
    };
    assert!(reason.contains("claims-our-name"), "reason: {reason}");
}
