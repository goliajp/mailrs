//! A message moved out of a mailbox is reported vanished to a QRESYNC
//! client that was not connected when it went.

use super::{Imap, PASSWORD, USER, directory, inbox, state, tagged};

#[tokio::test]
async fn a_moved_message_is_vanished_from_the_source() {
    let tmp = tempfile::tempdir().unwrap();
    let home = inbox(
        tmp.path(),
        &["new/1700000001.M1P1.h", "new/1700000002.M2P1.h"],
    );
    for leaf in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(home.join(".Trash").join(leaf)).unwrap();
    }
    let dir = directory(state(), tmp.path());
    let mut c = Imap::connect(&dir).await;
    c.cmd(&format!("LOGIN {USER} \"{PASSWORD}\"")).await;
    let r = c.cmd("SELECT INBOX").await;
    let uidvalidity = r
        .split("[UIDVALIDITY ")
        .nth(1)
        .and_then(|s| s.split(']').next())
        .unwrap()
        .to_string();
    let r = c.cmd("UID FETCH 1:* (UID)").await;
    assert!(r.contains("UID 1)") && r.contains("UID 2)"), "{r}");
    let r = c.cmd("UID MOVE 1 Trash").await;
    assert!(r.contains("* 1 EXPUNGE\r\n"), "{r}");
    assert!(tagged(&r).contains("OK MOVE completed"), "{r}");

    let mut later = Imap::connect(&dir).await;
    later.cmd(&format!("LOGIN {USER} \"{PASSWORD}\"")).await;
    later.cmd("ENABLE QRESYNC").await;
    let r = later
        .cmd(&format!("SELECT INBOX (QRESYNC ({uidvalidity} 1))"))
        .await;
    assert!(r.contains("* VANISHED (EARLIER) 1\r\n"), "{r}");
}
