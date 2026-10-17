//! Request fixtures for `/api/mock/*`: the same contract as
//! `request_contract.rs`, whose `every_fixture_has_a_test` lists these
//! fixtures too, so one cannot land here unread.

mod common;
use common::parse;
use mailrs_webapi::handlers;

#[test]
fn mock_namespace_create_body_matches() {
    let v: handlers::mock::rest::CreateNamespace = parse("mock-namespace-create");
    assert_eq!(v.name, "insight");
}

#[test]
fn mock_address_create_body_matches() {
    let v: handlers::mock::rest::NewAddress = parse("mock-address-create");
    assert_eq!(v.tag.as_deref(), Some("signup-0417"));
    // the client sends `{}` when no tag is given; the server then picks one
    let none: handlers::mock::rest::NewAddress = serde_json::from_str("{}").expect("empty body");
    assert_eq!(none.tag, None);
}

#[test]
fn mock_webhook_set_body_matches() {
    let v: handlers::mock::rest::SetWebhook = parse("mock-webhook-set");
    assert_eq!(v.url, "https://example.com/hooks/mock");
}

#[test]
fn mock_send_body_matches() {
    let v: handlers::mock::rest::SendBody = parse("mock-send");
    assert_eq!(v.from_tag.as_deref(), Some("sender"));
    assert_eq!(
        v.to,
        ["insight.signup-0417@mock.golia.jp", "someone@example.com"]
    );
    assert_eq!(v.subject, "Your verification code");
    assert_eq!(v.text, "Your code is 482913");
    assert_eq!(v.html.as_deref(), Some("<p>Your code is 482913</p>"));
}

#[test]
fn mock_settings_save_body_matches() {
    use mailrs_core_sidestate::families::mock::{CreatePolicy, Settings};
    let v: Settings = parse("mock-settings-save");
    assert_eq!(v.create, CreatePolicy::AdminsOnly);
    assert_eq!((v.retention_days, v.max_messages), (7, 200));
    assert_eq!((v.max_total_mb, v.max_namespaces_per_user), (1024, 5));
    assert!(v.enabled && v.invalid().is_none());
}
