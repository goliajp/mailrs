//! A message MOVEd into Trash puts its conversation in the Trash, also when
//! the uid index cannot name it: production has rows indexed with uid 0,
//! and the first MOVE on production answered `NO [TRYCREATE]` for one.

use super::{Imap, PASSWORD, USER, directory, inbox, state, tagged};

#[tokio::test]
async fn move_to_trash_finds_the_conversation_without_the_uid_index() {
    let tmp = tempfile::tempdir().unwrap();
    let home = inbox(tmp.path(), &["new/1700000001.M1P1.h"]);
    let dir = directory(state(), tmp.path());
    let bytes = std::fs::read(home.join("new/1700000001.M1P1.h")).unwrap();
    crate::ingest_delivered_file(&dir.state, USER, "1700000001.M1P1.h", &bytes, "INBOX", None);
    // as on production: the conversation is indexed, the uid is not
    let uids = mailrs_mailbox_kevy::keys::user_msg_by_uid(USER);
    dir.state
        .mailbox
        .store_ref()
        .del(&[uids.as_bytes()])
        .unwrap();

    let mut c = Imap::connect(&dir).await;
    c.cmd(&format!("LOGIN {USER} \"{PASSWORD}\"")).await;
    c.cmd("SELECT INBOX").await;
    let r = c.cmd("MOVE 1 Trash").await;
    assert!(tagged(&r).contains("OK MOVE completed"), "{r}");
    assert_eq!(dir.state.mailbox.trashed_threads(USER).unwrap().len(), 1);
    let r = c.cmd("SELECT INBOX").await;
    assert!(r.contains("* 0 EXISTS"), "{r}");
    let r = c.cmd("SELECT Trash").await;
    assert!(r.contains("* 1 EXISTS"), "{r}");
    assert!(
        home.join("new/1700000001.M1P1.h").exists(),
        "the file moved"
    );
}
