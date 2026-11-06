//! Request fixtures for team inboxes: the same contract as
//! `request_contract.rs`; `every_fixture_has_a_test` in
//! `request_contract_registry.rs` lists these fixtures too.

mod common;
use common::parse;
use mailrs_webapi::handlers;
use mailrs_webapi::handlers::shared_inbox as team;

#[test]
fn team_inbox_bodies_match() {
    let v: team::AssignBody = parse("shared-assign");
    assert_eq!(v.to.as_deref(), Some("bob@example.test"));
    let v: team::notes::NoteBody = parse("shared-note");
    assert_eq!(v.text, "Refund approved, call them back");
    let v: team::PresenceBody = parse("shared-presence");
    assert_eq!(
        v.state,
        mailrs_core_sidestate::families::shared_inbox::presence::State::Composing
    );
    let v: team::drafts::AnnounceBody = parse("shared-draft-announce");
    assert_eq!(v.draft_id, 7);
    let v: team::SharedFlag = parse("email-group-shared-set");
    assert!(v.shared);
}

#[test]
fn a_team_reply_names_what_it_saw() {
    let v: handlers::compose::SendRequest = parse("send-team-reply");
    let s = v.shared.expect("shared");
    assert_eq!((s.gid, s.conv, s.seen_seq, s.force), (3, 41, 2, false));
}
