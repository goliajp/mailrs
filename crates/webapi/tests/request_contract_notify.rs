//! Request fixtures for sender rules, push settings and Web Push: the same
//! contract as `request_contract.rs`, whose `every_fixture_has_a_test`
//! lists these fixtures too.

mod common;
use common::parse;
use mailrs_core_sidestate::families::notify::{Mode, Prefs};
use mailrs_core_sidestate::families::sender_rules::Disposal;
use mailrs_webapi::handlers;

#[test]
fn notify_prefs_set_body_matches() {
    let v: Prefs = parse("notify-prefs-set");
    assert_eq!(v.mode, Mode::Important);
    assert_eq!((v.quiet_start, v.quiet_end), (Some(1320), Some(420)));
    assert_eq!(v.utc_offset_minutes, 540);
    assert!(v.hide_content);
}

#[test]
fn notify_override_body_matches() {
    let v: handlers::notify::OverrideBody = parse("notify-override");
    assert_eq!(v.value.as_deref(), Some("always"));
}

#[test]
fn important_set_body_matches() {
    let v: handlers::notify::ImportantBody = parse("important-set");
    assert!(v.important);
}

#[test]
fn sender_bodies_match() {
    let v: handlers::sender_rules::ReclassifyBody = parse("sender-reclassify");
    assert_eq!(
        (v.sender.as_str(), v.bucket.as_str()),
        ("news@shop.test", "notifications")
    );
    let r: handlers::sender_rules::RuleBody = parse("sender-rule-set");
    assert_eq!(r.bucket.as_deref(), Some("promotions"));
    assert_eq!((r.keep_latest, r.older_than_days), (Some(3), Some(30)));
    assert_eq!(r.disposal, Disposal::Trash);
}

#[test]
fn web_push_bodies_match() {
    let v: handlers::web_push::SubscribeBody = parse("web-push-subscribe");
    assert!(
        handlers::web_push::acceptable(&v),
        "the fixture is a subscription the server stores"
    );
    let u: handlers::web_push::UnsubscribeBody = parse("web-push-unsubscribe");
    assert_eq!(u.endpoint, v.endpoint);
}
