//! The swipe setting's request fixture: the same contract as
//! `request_contract.rs`; `every_fixture_has_a_test` in
//! `request_contract_registry.rs` lists it too.

mod common;
use common::parse;
use mailrs_core_sidestate::families::swipe::{Swipe, SwipeAction};

#[test]
fn swipe_body_matches() {
    let v: Swipe = parse("swipe-set");
    assert_eq!((v.left, v.right), (SwipeAction::None, SwipeAction::Read));
}
