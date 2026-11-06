//! Request fixtures for drafts with files: the same contract as
//! `request_contract.rs`; `every_fixture_has_a_test` in
//! `request_contract_registry.rs` lists these fixtures too.

mod common;
use common::parse;
use mailrs_core_api::method::admin::SaveDraftRequest;

/// The autosave of a draft with files: each named by its upload's id.
#[test]
fn draft_save_with_attachments_body_matches() {
    let v: SaveDraftRequest = parse("draft-save-attachments");
    let files = v.attachments.expect("attachments");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "quote.pdf");
    assert_eq!(files[0].content_type, "application/pdf");
    assert_eq!(files[0].size, 48213);
    assert!(
        mailrs_core_sidestate::families::uploads::sha_of(&files[0].blob_id).is_some(),
        "{}",
        files[0].blob_id
    );
}

/// A client that does not know about files sends no `attachments`, which
/// keeps the draft's files.
#[test]
fn a_draft_save_without_attachments_keeps_them() {
    let v: SaveDraftRequest = parse("draft-save");
    assert_eq!(v.attachments, None);
}
