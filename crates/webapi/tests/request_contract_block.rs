//! Request fixtures for blocking a sender: the same contract as
//! `request_contract.rs`; `every_fixture_has_a_test` in
//! `request_contract_registry.rs` lists these fixtures too.

mod common;
use common::parse;
use mailrs_webapi::handlers::block_sender;

#[test]
fn block_sender_body_matches() {
    let v: block_sender::BlockSenderRequest = parse("block-sender");
    assert_eq!(v.address, "friend@example.com");
}

/// `allow` is what puts the address back on the whitelist on undo; a
/// misspelt field would default to false and lose it silently.
#[test]
fn unblock_sender_body_matches() {
    let v: block_sender::UnblockSenderRequest = parse("unblock-sender");
    assert_eq!(v.address, "friend@example.com");
    assert!(v.allow);
}
