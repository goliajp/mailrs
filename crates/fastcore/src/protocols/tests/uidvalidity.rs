//! A mailbox keeps one UIDVALIDITY and one vanished log however the
//! client spells its name.

use super::{Imap, PASSWORD, USER, directory, inbox, state};

#[tokio::test]
async fn inbox_in_any_case_is_one_mailbox() {
    let tmp = tempfile::tempdir().unwrap();
    inbox(tmp.path(), &[]);
    let dir = directory(state(), tmp.path());
    let store = dir.state.mailbox.store_ref();
    let key = format!("mailrs:user:{USER}:imap:uidvalidity:INBOX");
    store.set(key.as_bytes(), b"42").unwrap();
    crate::imap::backend::record_vanished(&dir.state, USER, "INBOX", 9, 5);

    let mut c = Imap::connect(&dir).await;
    c.cmd(&format!("LOGIN {USER} \"{PASSWORD}\"")).await;
    c.cmd("ENABLE QRESYNC").await;
    let r = c.cmd("SELECT inbox (QRESYNC (42 1))").await;

    assert!(r.contains("[UIDVALIDITY 42]"), "{r}");
    assert!(r.contains("* VANISHED (EARLIER) 9\r\n"), "{r}");
}
