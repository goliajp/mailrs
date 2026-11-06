//! The request fixtures of the settings every client reads (the swipe and
//! the sectioned inbox): the same contract as `request_contract.rs`, and
//! `request_contract_registry.rs` lists them too.

mod common;
use common::parse;
use mailrs_core_sidestate::families::sectioned_inbox::SectionedInbox;
use mailrs_core_sidestate::families::swipe::{Swipe, SwipeAction};

#[test]
fn swipe_body_matches() {
    let v: Swipe = parse("swipe-set");
    assert_eq!((v.left, v.right), (SwipeAction::None, SwipeAction::Read));
}

#[test]
fn sectioned_inbox_body_matches() {
    let v: SectionedInbox = parse("sectioned-inbox-set");
    assert!(!v.enabled);
}
