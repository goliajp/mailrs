//! Authenticated list mail is not junked for looking like bulk mail.

use super::tests::baseline_input;
use super::*;

/// A newsletter as the receiver scores it today: DMARC passes, it has an
/// unsubscribe link, the classifier is near certain (4.0) and the format
/// rules add 2.0 (`excessive_urls`).
fn newsletter() -> PipelineInput {
    let mut i = baseline_input();
    i.list_mail = true;
    i.ai_score = 4.0;
    i.content_score = 2.0;
    i.format_score = 2.0;
    i.matched_rules = vec!["excessive_urls".into()];
    i
}

fn junked(i: &PipelineInput) -> bool {
    matches!(make_delivery_decision(i), DeliveryDecision::Junk { .. })
}

#[test]
fn an_authenticated_newsletter_reaches_the_reader() {
    assert!(!junked(&newsletter()));
}

#[test]
fn the_classifier_alone_cannot_junk_authenticated_list_mail() {
    let mut i = newsletter();
    i.content_score = 0.0;
    i.format_score = 0.0;
    i.ai_score = 40.0;
    assert!(!junked(&i));
    assert_eq!(counted(&i).classifier, 2.5);
}

#[test]
fn a_ham_reading_is_not_raised_by_the_cap() {
    let mut i = newsletter();
    i.ai_score = -1.0;
    assert_eq!(counted(&i).classifier, -1.0);
}

/// The same newsletter without the DMARC pass is scored as before: the
/// header alone is something anyone can write.
#[test]
fn without_a_dmarc_pass_the_list_header_changes_nothing() {
    for dmarc in ["none", "fail", "temperror"] {
        let mut i = newsletter();
        i.auth.dmarc = dmarc.into();
        assert!(junked(&i), "dmarc={dmarc}");
        assert_eq!(counted(&i).content, 2.0);
    }
}

#[test]
fn without_a_list_header_a_dmarc_pass_changes_nothing() {
    let mut i = newsletter();
    i.list_mail = false;
    assert!(junked(&i));
}

/// What the message says still counts: money with urgency and a
/// shortened link are not format, and beside the capped classifier they
/// reach the threshold.
#[test]
fn content_rules_about_what_it_says_still_count() {
    let mut i = newsletter();
    i.content_score = 2.0 + 2.0 + 1.5; // excessive_urls, money_urgency, shortener_url
    assert!(junked(&i));
    let c = counted(&i);
    assert_eq!(c.content, 3.5);
    assert_eq!(c.content + c.classifier, 6.0);
}

#[test]
fn fraud_rules_still_count() {
    let mut i = newsletter();
    i.fraud.push(mailrs_fraud::Finding::scored(
        "subject-claims-brand",
        mailrs_fraud::Layer::Identity,
        4.5,
        "a brand in the subject",
    ));
    assert!(junked(&i));
}

/// The stored reason says what counted, so a later reader of the verdict
/// can tell a classifier decision from a content one.
#[test]
fn the_junk_reason_carries_what_counted() {
    let mut i = newsletter();
    i.content_score = 5.5;
    let DeliveryDecision::Junk { why, .. } = make_delivery_decision(&i) else {
        panic!("junked");
    };
    assert_eq!(why.params["content"], "3.5");
    assert_eq!(why.params["classifier"], "2.5");
}
