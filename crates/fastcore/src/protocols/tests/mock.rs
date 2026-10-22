//! An IMAP client logs in as `{ns}@{mock domain}` with the namespace's
//! SMTP token and reads the namespace's inbound mail, read-only, over a
//! real session. A real account signs in as before.
//!
//! One test, and the only one in this binary that sets the mock
//! variables: the mock store reads them once per process.

use std::sync::Arc;
use std::time::Duration;

use kevy_client::Connection;
use mailrs_core_sidestate::families::mock;

use super::{Imap, tagged};
use crate::FastcoreState;
use crate::protocols::FastcoreDirectory;

const KEVY: &str = "mem://imap-reads-a-mock-namespace";
const DOMAIN: &str = "mock.example.test";

fn store(kv: &mut Connection, ns: &str, direction: mock::Direction, body: &str) {
    let meta = mock::MessageMeta {
        id: mock::fresh_message_id(),
        namespace: ns.into(),
        tag: String::new(),
        direction,
        rcpt: format!("{ns}@{DOMAIN}"),
        size: 0,
        received_at: 0,
        uid: 0,
        summary: Default::default(),
    };
    let raw = format!("From: a@example.org\r\nSubject: {body}\r\n\r\n{body}\r\n");
    let stored = mock::store_message(kv, mock::root(), meta, raw.as_bytes()).unwrap();
    assert_eq!(stored, mock::Stored::Stored);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_mock_namespace_reads_over_imap_and_an_account_still_signs_in() {
    let tmp = tempfile::tempdir().unwrap();
    // SAFETY: no other test in this binary reads or writes these
    unsafe {
        std::env::set_var("MAILRS_MOCK_DOMAIN", DOMAIN);
        std::env::set_var("MAILRS_MOCK_ROOT", tmp.path().join("mock"));
    }

    let mut kv = Connection::connect(KEVY).unwrap();
    let created = mock::create_namespace(&mut kv, "dev@golia.example.test", "app").unwrap();
    let token = mock::rotate_smtp_token(&mut kv, "app").unwrap();
    store(&mut kv, "app", mock::Direction::In, "first");
    store(&mut kv, "app", mock::Direction::In, "second");
    store(
        &mut kv,
        "app",
        mock::Direction::CapturedOut,
        "sent by the app",
    );
    store(&mut kv, "app", mock::Direction::In, "third");

    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    let hash = mailrs_core::users::UserStore::hash_password("correct horse").unwrap();
    mailbox
        .upsert_account(
            "alice@golia.example.test",
            &serde_json::json!({ "password_hash": hash }).to_string(),
        )
        .unwrap();
    let mut state = FastcoreState::new(mailbox);
    state.net_url = Some(KEVY.to_string());
    let state = Arc::new(FastcoreDirectory {
        state: Arc::new(state),
        maildir_root: tmp.path().join("maildir"),
    });

    // 1. only the namespace's token, and only the bare address
    let mut c = Imap::connect(&state).await;
    let r = c.cmd(&format!("LOGIN app@{DOMAIN} nope")).await;
    assert!(tagged(&r).contains(" NO "), "{r}");
    let r = c.cmd(&format!("LOGIN app.signup@{DOMAIN} {token}")).await;
    assert!(tagged(&r).contains(" NO "), "{r}");
    let r = c.cmd(&format!("LOGIN app@{DOMAIN} {token}")).await;
    assert!(tagged(&r).contains(" OK "), "{r}");

    // 2. one read-only INBOX holding the inbound mail, ascending by uid
    let r = c.cmd("LIST \"\" \"*\"").await;
    assert_eq!(r.matches("* LIST").count(), 1, "{r}");
    assert!(r.contains("INBOX"), "{r}");
    let r = c.cmd("SELECT INBOX").await;
    assert!(r.contains("* 3 EXISTS"), "{r}");
    assert!(tagged(&r).contains("OK [READ-ONLY]"), "{r}");
    let uidvalidity = created.created_at / 1000;
    assert!(r.contains(&format!("[UIDVALIDITY {uidvalidity}]")), "{r}");
    assert!(r.contains("[UIDNEXT 5]"), "{r}");
    let r = c.cmd("UID FETCH 1:* (UID FLAGS)").await;
    for (seq, uid) in [(1, 1), (2, 2), (3, 4)] {
        assert!(
            r.contains(&format!("* {seq} FETCH (UID {uid} FLAGS ())")),
            "{r}"
        );
    }
    assert!(!r.contains("UID 3 "), "captured outbound mail listed: {r}");

    // 3. the stored file comes back, and reading it marks nothing
    let r = c.cmd("FETCH 3 BODY[]").await;
    assert!(r.contains("Subject: third\r\n\r\nthird\r\n"), "{r}");
    let r = c.cmd("FETCH 3 (FLAGS)").await;
    assert!(r.contains("* 3 FETCH (FLAGS ())"), "{r}");

    // 4. nothing changes it
    for line in [
        "STORE 1 +FLAGS (\\Seen)",
        "UID STORE 1 +FLAGS (\\Deleted)",
        "COPY 1 INBOX",
        "UID MOVE 1 Trash",
        "EXPUNGE",
        "APPEND INBOX {5}",
        "CREATE Work",
        "DELETE INBOX",
    ] {
        let r = c.cmd(line).await;
        assert!(tagged(&r).contains(" NO [READ-ONLY]"), "{line}: {r}");
    }
    assert_eq!(
        std::fs::read_dir(mock::root().join("app")).unwrap().count(),
        4
    );
    let r = c.cmd("UID SEARCH ALL").await;
    assert!(r.contains("* SEARCH 1 2 4\r\n"), "{r}");
    c.cmd("LOGOUT").await;

    // 5. a namespace created again under the same name is a new mailbox
    tokio::time::sleep(Duration::from_millis(1100)).await;
    mock::delete_namespace(&mut kv, mock::root(), "app").unwrap();
    mock::create_namespace(&mut kv, "dev@golia.example.test", "app").unwrap();
    let r = Imap::connect(&state)
        .await
        .cmd(&format!("LOGIN app@{DOMAIN} {token}"))
        .await;
    assert!(
        tagged(&r).contains(" NO "),
        "the old token still works: {r}"
    );
    let token = mock::rotate_smtp_token(&mut kv, "app").unwrap();
    let mut c = Imap::connect(&state).await;
    c.cmd(&format!("LOGIN app@{DOMAIN} {token}")).await;
    let r = c.cmd("EXAMINE INBOX").await;
    assert!(r.contains("* 0 EXISTS"), "{r}");
    assert!(!r.contains(&format!("[UIDVALIDITY {uidvalidity}]")), "{r}");

    // 6. a real account signs in and selects read-write, as before
    let mut c = Imap::connect(&state).await;
    let r = c.cmd("LOGIN alice@golia.example.test wrong").await;
    assert!(tagged(&r).contains(" NO "), "{r}");
    let r = c
        .cmd("LOGIN alice@golia.example.test \"correct horse\"")
        .await;
    assert!(tagged(&r).contains(" OK "), "{r}");
    let r = c.cmd("SELECT INBOX").await;
    assert!(tagged(&r).contains("OK [READ-WRITE]"), "{r}");
}
