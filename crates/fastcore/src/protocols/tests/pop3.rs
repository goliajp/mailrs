//! POP3 over fastcore: messages are numbered from 1 whatever their uids,
//! and a deletion that fails at QUIT is reported.

use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::{PASSWORD, USER, directory, inbox, state};
use crate::protocols::FastcoreDirectory;

/// Send `lines` after signing in; every reply line, the sign-in's first.
async fn conversation(dir: Arc<FastcoreDirectory>, lines: &str, replies: usize) -> Vec<String> {
    let (client, server) = tokio::io::duplex(1 << 16);
    tokio::spawn(mailrs_mail_protocols::pop3::session(dir, server, None));
    let (r, mut w) = tokio::io::split(client);
    let mut r = BufReader::new(r);
    w.write_all(format!("USER {USER}\r\nPASS {PASSWORD}\r\n{lines}").as_bytes())
        .await
        .unwrap();
    let mut out = Vec::new();
    for _ in 0..replies + 3 {
        let mut l = String::new();
        r.read_line(&mut l).await.unwrap();
        out.push(l);
    }
    out.split_off(2)
}

fn uids_from_four() -> crate::FastcoreState {
    let st = state();
    // uids 1..=3 go to mail elsewhere, so this mailbox's uids start at 4
    for i in 0..3 {
        st.mailbox
            .allocate_uid(USER, &format!("other{i}@test"))
            .unwrap();
    }
    st
}

#[tokio::test]
async fn messages_are_numbered_from_one() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = inbox(
        tmp.path(),
        &["new/1700000001.M1P1.h", "new/1700000002.M2P1.h"],
    );
    let fc = directory(uids_from_four(), tmp.path());
    let out = conversation(fc, "LIST\r\nRETR 4\r\nDELE 2\r\nQUIT\r\n", 7).await;
    assert_eq!(
        out,
        [
            "+OK 2 messages\r\n",
            "+OK maildrop follows\r\n",
            "1 45\r\n",
            "2 45\r\n",
            ".\r\n",
            "-ERR no such message\r\n",
            "+OK marked for deletion\r\n",
            "+OK bye\r\n",
        ]
    );
    assert!(!dir.join("new/1700000002.M2P1.h").exists());
    assert!(dir.join("new/1700000001.M1P1.h").exists());
}

#[tokio::test]
async fn a_deletion_that_fails_at_quit_is_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = inbox(tmp.path(), &["new/1700000001.M1P1.h"]);
    let fc = directory(state(), tmp.path());
    let stuck = dir.join("new/1700000001.M1P1.h");
    let (client, server) = tokio::io::duplex(1 << 16);
    tokio::spawn(mailrs_mail_protocols::pop3::session(fc, server, None));
    let (r, mut w) = tokio::io::split(client);
    let mut r = BufReader::new(r);
    w.write_all(format!("USER {USER}\r\nPASS {PASSWORD}\r\nDELE 1\r\n").as_bytes())
        .await
        .unwrap();
    for _ in 0..4 {
        r.read_line(&mut String::new()).await.unwrap();
    }
    // a directory in the file's place: unlink fails, for root as well
    std::fs::remove_file(&stuck).unwrap();
    std::fs::create_dir(&stuck).unwrap();
    w.write_all(b"QUIT\r\n").await.unwrap();
    let mut l = String::new();
    r.read_line(&mut l).await.unwrap();
    assert_eq!(l, "-ERR some deleted messages not removed\r\n");
}
