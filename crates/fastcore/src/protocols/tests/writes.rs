//! A write the Maildir refused is answered NO, not reported as done.

use super::{Imap, PASSWORD, USER, directory, inbox, state, tagged};

async fn selected(root: &std::path::Path) -> Imap {
    let mut c = Imap::connect(&directory(state(), root)).await;
    c.cmd(&format!("LOGIN {USER} \"{PASSWORD}\"")).await;
    let r = c.cmd("SELECT INBOX").await;
    assert!(tagged(&r).contains("OK [READ-WRITE]"), "{r}");
    c
}

#[tokio::test]
async fn a_store_whose_file_is_gone_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = inbox(tmp.path(), &["new/1700000001.M1P1.h"]);
    let mut c = selected(tmp.path()).await;
    std::fs::remove_file(dir.join("new/1700000001.M1P1.h")).unwrap();

    let r = c.cmd("STORE 1 +FLAGS (\\Seen)").await;

    assert!(tagged(&r).starts_with("a3 NO STORE failed"), "{r}");
    assert!(!r.contains("FETCH"), "{r}");
}

#[tokio::test]
async fn an_expunge_that_cannot_delete_is_refused_and_not_announced() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = inbox(
        tmp.path(),
        &["cur/1700000001.M1P1.h:2,T", "cur/1700000002.M2P1.h:2,T"],
    );
    let mut c = selected(tmp.path()).await;
    // a directory in the file's place: unlink fails, for root as well
    let stuck = dir.join("cur/1700000002.M2P1.h:2,T");
    std::fs::remove_file(&stuck).unwrap();
    std::fs::create_dir(&stuck).unwrap();

    let r = c.cmd("EXPUNGE").await;

    // highest seqno first: the stuck one is tried first and stops the rest
    assert!(r.starts_with("a3 NO EXPUNGE failed"), "{r}");
    assert!(dir.join("cur/1700000001.M1P1.h:2,T").exists());
}

#[tokio::test]
async fn an_expunge_of_a_file_already_gone_is_announced() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = inbox(tmp.path(), &["cur/1700000001.M1P1.h:2,T"]);
    let mut c = selected(tmp.path()).await;
    std::fs::remove_file(dir.join("cur/1700000001.M1P1.h:2,T")).unwrap();

    let r = c.cmd("EXPUNGE").await;

    assert_eq!(r, "* 1 EXPUNGE\r\na3 OK EXPUNGE completed\r\n");
}
