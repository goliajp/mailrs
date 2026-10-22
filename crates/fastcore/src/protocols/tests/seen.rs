//! A body read over IMAP marks the message read where STORE does: the
//! file name and the index the conversation views read.

use mailrs_core_api::method::message::{MessageWire, maildir_flags_to_bitmask};
use mailrs_maildir::Flag;

use super::{Imap, PASSWORD, USER, directory, inbox, state, tagged};
use crate::imap::backend;

#[tokio::test]
async fn a_body_read_marks_the_file_and_the_index_read() {
    let tmp = tempfile::tempdir().unwrap();
    let home = inbox(tmp.path(), &[]);
    let dir = directory(state(), tmp.path());
    let mb = backend::get_mailbox_in(tmp.path(), USER, "INBOX").unwrap();
    let raw = b"From: bob@x.test\r\nSubject: hi\r\nMessage-ID: <read@test>\r\n\r\nbody\r\n";
    let uid = backend::append(&dir.state, USER, &mb, raw).unwrap();
    let seen = maildir_flags_to_bitmask(&[Flag::Seen]);
    let index_flags = || {
        let bytes = dir.state.mailbox.get_message_by_uid(USER, uid).unwrap();
        serde_json::from_slice::<MessageWire>(&bytes.unwrap())
            .unwrap()
            .flags
    };
    assert_eq!(index_flags() & seen, 0, "arrives unread");

    let mut c = Imap::connect(&dir).await;
    c.cmd(&format!("LOGIN {USER} \"{PASSWORD}\"")).await;
    c.cmd("SELECT INBOX").await;
    let r = c.cmd("FETCH 1 (BODY[])").await;

    assert!(r.contains(" FLAGS (\\Seen))\r\n"), "{r}");
    assert!(tagged(&r).contains("OK FETCH completed"), "{r}");
    let names: Vec<String> = std::fs::read_dir(home.join("cur"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.len() == 1 && names[0].ends_with(":2,S"), "{names:?}");
    assert_ne!(index_flags() & seen, 0, "the index still says unread");
}
