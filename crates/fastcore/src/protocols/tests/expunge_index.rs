//! A message an IMAP client expunges leaves the index with its file: the
//! conversation lists one message fewer, and goes when it was the last.
//! Until this, the file went and the row stayed, so the web went on
//! listing a message nobody could open.

use std::path::Path;
use std::sync::Arc;

use super::{Imap, PASSWORD, USER, directory, state, tagged};
use crate::protocols::FastcoreDirectory;

const FIRST: &str = "Message-ID: <first@test>\r\nFrom: Ann <ann@far.test>\r\n\
To: alice@golia.example.test\r\nSubject: plans\r\n\
Date: Tue, 14 Nov 2023 10:00:00 +0000\r\n\r\nshall we?\r\n";
const SECOND: &str = "Message-ID: <second@test>\r\nIn-Reply-To: <first@test>\r\n\
References: <first@test>\r\nFrom: Ann <ann@far.test>\r\n\
To: alice@golia.example.test\r\nSubject: Re: plans\r\n\
Date: Tue, 14 Nov 2023 11:00:00 +0000\r\n\r\nstill on?\r\n";

/// `files` (name, bytes) delivered into the INBOX and indexed as the
/// drain indexes them.
fn delivered(root: &Path, files: &[(&str, &str)]) -> Arc<FastcoreDirectory> {
    let home = root.join("golia.example.test").join("alice");
    for leaf in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(home.join(leaf)).unwrap();
    }
    let dir = directory(state(), root);
    for (name, raw) in files {
        std::fs::write(home.join("new").join(name), raw).unwrap();
        crate::ingest_delivered_file(&dir.state, USER, name, raw.as_bytes(), "INBOX", None);
    }
    dir
}

async fn expunge_first(dir: &Arc<FastcoreDirectory>) {
    let mut c = Imap::connect(dir).await;
    c.cmd(&format!("LOGIN {USER} \"{PASSWORD}\"")).await;
    c.cmd("SELECT INBOX").await;
    c.cmd("STORE 1 +FLAGS.SILENT (\\Deleted)").await;
    let r = c.cmd("EXPUNGE").await;
    assert!(tagged(&r).contains("OK EXPUNGE completed"), "{r}");
}

#[tokio::test]
async fn an_expunged_message_leaves_its_conversation() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = delivered(
        tmp.path(),
        &[("1700000001.M1P1.h", FIRST), ("1700000002.M2P1.h", SECOND)],
    );
    let tid = "first@test";
    assert_eq!(
        dir.state
            .mailbox
            .user_thread_message_ids(USER, tid)
            .unwrap(),
        vec!["first@test", "second@test"]
    );

    expunge_first(&dir).await;

    assert_eq!(
        dir.state
            .mailbox
            .user_thread_message_ids(USER, tid)
            .unwrap(),
        vec!["second@test"]
    );
    let row = dir
        .state
        .mailbox
        .get_thread_for_user(USER, tid)
        .unwrap()
        .expect("the conversation stays");
    assert_eq!(row.count, 1);
    assert!(
        dir.state
            .mailbox
            .user_message_view(USER, "first@test")
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn expunging_the_only_message_takes_the_conversation() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = delivered(tmp.path(), &[("1700000001.M1P1.h", FIRST)]);
    assert!(
        dir.state
            .mailbox
            .get_thread_for_user(USER, "first@test")
            .unwrap()
            .is_some()
    );

    expunge_first(&dir).await;

    assert!(
        dir.state
            .mailbox
            .get_thread_for_user(USER, "first@test")
            .unwrap()
            .is_none()
    );
}
