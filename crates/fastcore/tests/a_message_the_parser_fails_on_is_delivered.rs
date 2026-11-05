//! A message the parse worker fails on reaches the inbox like any other,
//! through the receiver's scan and the drain both, and the failure shows
//! on the core's health answer; a worker that is merely unavailable
//! changes nothing but the readings it would have given.
//!
//! The parsers run in process here, failing on the inputs that carry the
//! fault header (`mailrs_parse_worker::install_faults`); the real worker
//! dying is tested against its binary and by the release smoke.

// shared with the mock-namespace tests; this one needs only its SMTP client
#[allow(dead_code)]
mod mock_support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use mailrs_core_api::server::Handler;
use mock_support::Smtp;

const KEVY: &str = "mem://fastcore-parser-fails-on-a-message";
const DOMAIN: &str = "golia.example.test";
const BOB: &str = "bob@golia.example.test";

/// An ordinary letter: dated, text and HTML alternatives, nothing the
/// content rules score.
fn letter(id: &str, fault: &str) -> String {
    format!(
        "Message-ID: <{id}@shop.example.test>\r\nFrom: Billing <billing@shop.example.test>\r\n\
To: {BOB}\r\nDate: Thu, 5 Nov 2026 09:00:00 +0900\r\nSubject: invoice {id}\r\n\
X-Parse-Worker-Fault: {fault}\r\nMIME-Version: 1.0\r\n\
Content-Type: multipart/alternative; boundary=b\r\n\r\n\
--b\r\nContent-Type: text/plain\r\n\r\nYour invoice for October is attached.\r\n\
--b\r\nContent-Type: text/html\r\n\r\n<p>Your invoice for October is attached.</p>\r\n\
--b--"
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_message_the_parser_fails_on_is_delivered_to_the_inbox() {
    mailrs_parse_worker::install_faults("fastcore");
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
        std::env::set_var("MAILRS_ANTISPAM_ENABLED", "true");
        std::env::set_var("MAILRS_FASTCORE_SPOOL_INTERVAL_SECS", "600");
        std::env::set_var("MAILRS_PORT", "0");
        std::env::set_var("MAILRS_SUBMISSION_PORT", "0");
        std::env::set_var("MAILRS_SMTPS_PORT", "0");
        std::env::set_var("MAILRS_HOSTNAME", "mx.example.test");
    }
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
    tokio::time::sleep(Duration::from_millis(500)).await;
    let before = Handler::healthz(state.as_ref())
        .await
        .parsing
        .expect("the parsing section")
        .failures_24h;

    let mut smtp = Smtp::connect(receiver.smtp.port()).await;
    smtp.cmd("EHLO sender.example.org").await;
    for (id, fault) in [("failed", "abort"), ("unavailable", "unavailable")] {
        let r = smtp
            .send("billing@shop.example.test", &[BOB], &letter(id, fault))
            .await;
        assert!(r.starts_with("250"), "{id}: accepted, not refused: {r}");
    }

    let sent = Instant::now();
    let rows = loop {
        let ids = state.mailbox.all_thread_ids_for_user(BOB).unwrap();
        if ids.len() == 2 {
            break ids;
        }
        assert!(
            sent.elapsed() < Duration::from_secs(10),
            "delivered {} of 2 within 10 s",
            ids.len()
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    for tid in rows {
        let row = state
            .mailbox
            .get_thread_for_user(BOB, &tid)
            .unwrap()
            .expect("bob's row");
        assert_ne!(row.category, "junk", "{}: not junked", row.subject);
        assert_ne!(row.category, "spam", "{}: not junked", row.subject);
        // the body could not be read, so the row has no preview
        assert_eq!(row.latest_preview, "", "{}", row.subject);
    }
    let parsing = Handler::healthz(state.as_ref())
        .await
        .parsing
        .expect("the parsing section");
    assert!(parsing.worker);
    // the receiver's scan and the drain each failed on the first letter;
    // the second was unavailable, which is not a failure
    assert!(
        parsing.failures_24h >= before + 2,
        "{} -> {}",
        before,
        parsing.failures_24h
    );
    assert!(parsing.last_failure_at.is_some());
    assert!(
        parsing
            .failures
            .iter()
            .any(|f| f.parser == "html2text" && f.kind == "panic")
    );
}
