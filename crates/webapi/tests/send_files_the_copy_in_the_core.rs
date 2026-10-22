//! The sender's copy, as the web tier hands it to the core after a send:
//! one file in Sent, read; the message in the Sent list; the usage counted
//! once, by the core; and every envelope recipient, Bcc included, in the
//! contacts.
//!
//! Called through the core client rather than `/api/mail/send`: the queue
//! write after it uses a pipeline, which the in-process kevy these tests
//! run on does not support.

mod live;

const KEVY: &str = "mem://webapi-send-files-the-copy-in-the-core";
const ALICE: &str = "alice@example.test";

const MESSAGE: &[u8] = b"Date: Tue, 14 Nov 2023 22:40:00 +0000\r\n\
From: Alice <alice@example.test>\r\n\
To: Bob <bob@partner.test>\r\n\
Subject: Plans\r\n\
Message-ID: <plans@example.test>\r\n\
\r\n\
Friday?\r\n";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_core_files_the_copy_and_counts_it_once() {
    let tmp = tempfile::tempdir().expect("tmp");
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
        std::env::set_var("MAILRS_MAILDIR", tmp.path());
    }
    let mut net = kevy_client::Connection::connect(KEVY).expect("kevy");
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;

    let rcpts = vec![
        "Bob <bob@partner.test>".to_string(),
        "dave@partner.test".to_string(),
    ];
    let filed = live
        .core
        .file_sent(ALICE, MESSAGE, &rcpts)
        .await
        .expect("filed");
    assert_eq!(filed.message_id, "plans@example.test");

    let cur = tmp.path().join("example.test/alice/.Sent/cur");
    let files: Vec<_> = std::fs::read_dir(&cur)
        .expect("Sent/cur")
        .map(|e| e.expect("entry").path())
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    let file = files[0].file_name().unwrap().to_string_lossy().into_owned();
    let (name, flags) = file.split_once(":2,").expect("flagged");
    assert_eq!(flags, "S", "the copy is not read on disk");
    assert_eq!(filed.blob_ref, format!(".Sent/{name}"));

    let listed = live
        .core
        .list_sent_messages(ALICE)
        .await
        .expect("sent list");
    assert!(
        listed
            .items
            .iter()
            .any(|m| m.message_id == filed.message_id),
        "the copy is not in the Sent list"
    );

    let used = net
        .get(mailrs_core_sidestate::keys::quota::used_key(ALICE).as_bytes())
        .expect("usage")
        .expect("usage counted");
    assert_eq!(
        String::from_utf8(used).unwrap(),
        MESSAGE.len().to_string(),
        "usage is not the copy's size counted once"
    );

    let contacts = net
        .hgetall(mailrs_core_sidestate::families::contact_cards::autocomplete_key(ALICE).as_bytes())
        .expect("contacts");
    assert!(
        contacts
            .iter()
            .any(|v| v.as_slice() == b"dave@partner.test"),
        "the Bcc recipient is not a contact"
    );
}
