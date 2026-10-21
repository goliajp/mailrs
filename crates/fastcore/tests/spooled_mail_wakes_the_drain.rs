//! Mail the receiver spools is delivered and indexed when the receiver
//! announces it, not on the drain's next poll. The poll here is ten
//! minutes, so only the announcement can deliver within the wait.

// shared with the mock-namespace tests; this one needs only its SMTP client
#[allow(dead_code)]
mod mock_support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use mock_support::Smtp;

const KEVY: &str = "mem://fastcore-spooled-mail-wakes-the-drain";
const DOMAIN: &str = "golia.example.test";
const BOB: &str = "bob@golia.example.test";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_receiver_announcing_a_spooled_message_wakes_the_drain() {
    let tmp = tempfile::tempdir().unwrap();
    let maildir = tmp.path().join("maildir");
    for leaf in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(maildir.join(DOMAIN).join("bob").join(leaf)).unwrap();
    }
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
        std::env::set_var("MAILRS_SPOOL_ROOT", tmp.path().join("spool"));
        std::env::set_var("MAILRS_MAILDIR", &maildir);
        std::env::set_var("MAILRS_LOCAL_DOMAINS", DOMAIN);
        std::env::set_var("MAILRS_ANTISPAM_ENABLED", "false");
        std::env::set_var("MAILRS_FASTCORE_SPOOL_INTERVAL_SECS", "600");
        std::env::set_var("MAILRS_PORT", "0");
        std::env::set_var("MAILRS_SUBMISSION_PORT", "0");
        std::env::set_var("MAILRS_SMTPS_PORT", "0");
        std::env::set_var("MAILRS_HOSTNAME", "mx.example.test");
    }
    // Hold the in-process kevy open before the receiver opens it: its first
    // opens race, and two racing opens of one mem:// name can each get a store.
    let _kevy = kevy_client::Connection::connect(KEVY).unwrap();
    let receiver = mailrs_receiver::start().await;

    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    mailbox.ensure_thread_table();
    mailbox.ensure_admin_indexes();
    mailbox.upsert_account(BOB, "{}").unwrap();
    let mut state = mailrs_fastcore::FastcoreState::new(mailbox);
    state.net_url = Some(KEVY.to_string());
    let state = Arc::new(state);
    tokio::spawn(mailrs_fastcore::spool_drain::spawn(state.clone()));
    // the drain's first tick runs at once and finds nothing; let it pass
    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut smtp = Smtp::connect(receiver.smtp.port()).await;
    smtp.cmd("EHLO sender.example.org").await;
    let r = smtp
        .send(
            "a@example.org",
            &[BOB],
            "Message-ID: <wake-1@example.org>\r\nSubject: hello\r\n\r\nhi",
        )
        .await;
    assert!(r.starts_with("250"), "{r}");

    let sent = Instant::now();
    while sent.elapsed() < Duration::from_secs(10) {
        if !state
            .mailbox
            .all_thread_ids_for_user(BOB)
            .unwrap()
            .is_empty()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the spooled message was not delivered within 10 s of being announced");
}
