//! Taking over a team reply that has files.

use std::path::Path;
use std::time::SystemTime;

use mailrs_core_api::method::admin::DraftAttachment;

use super::{conn, save_draft};
use crate::families::shared_inbox::{ConvRef, drafts, record_copy};
use crate::families::{draft_store, uploads};

fn no_link(_: &Path, _: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other("another volume"))
}

/// a@x writing the reply to conversation `r`, with one file.
fn with_a_file(c: &mut kevy_client::Connection, root: &Path, r: ConvRef) -> String {
    record_copy(c, r, "b@x", "t-b").unwrap();
    save_draft(c, "a@x", 1, "see attached");
    let uploads::Put::Stored(sha) =
        uploads::put(root, "a@x", b"the contract", SystemTime::now()).unwrap()
    else {
        panic!("not stored");
    };
    let mut d = draft_store::get(c, "a@x", 1).unwrap().unwrap();
    d.attachments = vec![DraftAttachment {
        blob_id: uploads::blob_id(&sha),
        name: "contract.pdf".into(),
        content_type: "application/pdf".into(),
        size: 12,
    }];
    draft_store::put(c, "a@x", &d).unwrap();
    drafts::announce(c, r, "a@x", 1, 10).unwrap().unwrap();
    sha
}

#[test]
fn the_taker_gets_files_of_their_own() {
    let mut c = conn("handover-link");
    let root = tempfile::tempdir().unwrap();
    let r = ConvRef { gid: 3, conv: 1 };
    let sha = with_a_file(&mut c, root.path(), r);
    let h = drafts::take_over(&mut c, root.path(), r, "b@x", 12)
        .unwrap()
        .unwrap();
    let copy = draft_store::get(&mut c, "b@x", h.draft_id)
        .unwrap()
        .unwrap();
    assert_eq!(copy.attachments.len(), 1);
    let theirs = uploads::path_of(root.path(), "b@x", &copy.attachments[0].blob_id).unwrap();
    assert_eq!(std::fs::read(&theirs).unwrap(), b"the contract");
    // the author throwing theirs away takes nothing from the taker
    draft_store::remove(&mut c, "a@x", 1).unwrap();
    uploads::purge_account(root.path(), "a@x", false).unwrap();
    assert!(!uploads::account_dir(root.path(), "a@x").exists());
    assert_eq!(std::fs::read(&theirs).unwrap(), b"the contract");
    assert!(theirs.ends_with(&sha));
}

#[test]
fn a_file_that_cannot_be_linked_is_copied() {
    let mut c = conn("handover-copy");
    let root = tempfile::tempdir().unwrap();
    let r = ConvRef { gid: 3, conv: 2 };
    let sha = with_a_file(&mut c, root.path(), r);
    let h = drafts::take_over_linking(&mut c, root.path(), r, "b@x", 12, no_link)
        .unwrap()
        .unwrap();
    let theirs = uploads::account_dir(root.path(), "b@x").join(&sha);
    assert_eq!(std::fs::read(&theirs).unwrap(), b"the contract");
    let copy = draft_store::get(&mut c, "b@x", h.draft_id)
        .unwrap()
        .unwrap();
    assert_eq!(copy.attachments[0].blob_id, uploads::blob_id(&sha));
}

#[test]
fn no_copy_is_made_when_a_file_cannot_be_given() {
    let mut c = conn("handover-fail");
    let root = tempfile::tempdir().unwrap();
    let r = ConvRef { gid: 3, conv: 3 };
    let sha = with_a_file(&mut c, root.path(), r);
    std::fs::remove_file(uploads::account_dir(root.path(), "a@x").join(&sha)).unwrap();
    assert!(drafts::take_over_linking(&mut c, root.path(), r, "b@x", 12, no_link).is_err());
    let theirs = crate::families::prefs::hgetall_values::<mailrs_core_api::method::admin::DraftWire>(
        &mut c,
        &crate::families::prefs::drafts_key("b@x"),
    );
    assert!(
        theirs.is_empty(),
        "a draft naming a file b@x lacks: {theirs:?}"
    );
    let p = drafts::pointer(&mut c, r).unwrap().unwrap();
    assert_eq!(p.author, "a@x", "the reply stays with its author");
    assert!(drafts::handoffs(&mut c, r).unwrap().is_empty());
}
