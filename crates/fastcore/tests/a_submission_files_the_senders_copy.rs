//! Mail submitted over SMTP AUTH leaves the sender with what a web send
//! leaves them: the copy in Sent, read, counted as sent in its
//! conversation. The same message submitted again (a client retrying)
//! files no second copy.
//!
//! One test, because the receiver reads its configuration from the
//! environment once per process. Against a kevy-server: the outbound job
//! for the remote recipient pipelines, which the in-process store does not.

// shared with the mock-namespace tests; this one needs only its SMTP client
#[allow(dead_code)]
mod mock_support;

use std::sync::Arc;

use base64::Engine;
use mailrs_core_api::method::admin::AddAccountRequest;
use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::types::{ConversationFilter, ConversationSummaryWire};
use mock_support::Smtp;

const SECRET: &str = "secret";
const BOB: &str = "bob@golia.example";
const PASSWORD: &str = "a-real-password-1";

fn message(id: &str, subject: &str) -> String {
    format!(
        "From: Bob <{BOB}>\r\nTo: someone@example.org\r\nSubject: {subject}\r\n\
         Message-ID: <{id}>\r\nDate: Wed, 5 Nov 2026 10:00:00 +0900\r\n\r\nhello"
    )
}

/// What a sender reads off a conversation row, apart from what names the
/// message itself.
fn sender_side(r: &ConversationSummaryWire) -> serde_json::Value {
    serde_json::json!({
        "message_count": r.message_count,
        "unread_count": r.unread_count,
        "sent_count": r.sent_count,
        "category": r.category,
        "archived": r.archived,
        "quarantined": r.quarantined,
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_submission_is_in_sent_like_a_web_send() {
    let (_kevy, url) = mailrs_test_docker::kevy::kevy_server().await;
    let tmp = tempfile::tempdir().unwrap();
    let spool = tmp.path().join("spool");
    let maildir = tmp.path().join("maildir");
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    let mut state = mailrs_fastcore::FastcoreState::new(mailbox);
    state.net_url = Some(url.clone());
    state.maildir_root = maildir.to_string_lossy().into_owned();
    let state = Arc::new(state);
    let router = mailrs_fastcore::build_router_with_secret(state.clone(), SECRET.into());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let core = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await });
    let client = mailrs_core_api::client::Client::new(core.clone(), SECRET);
    client
        .add_account(&AddAccountRequest {
            address: BOB.into(),
            display_name: String::new(),
            password: PASSWORD.into(),
            password_hash: None,
        })
        .await
        .unwrap();

    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", &url);
        std::env::set_var("MAILRS_CORE_RPC_BASE", &core);
        std::env::set_var("MAILRS_CORE_API_SECRET", SECRET);
        std::env::set_var("MAILRS_SPOOL_ROOT", &spool);
        std::env::set_var("MAILRS_MAILDIR", &maildir);
        std::env::set_var("MAILRS_LOCAL_DOMAINS", "golia.example");
        std::env::set_var("MAILRS_ANTISPAM_ENABLED", "false");
        std::env::set_var("MAILRS_PORT", "0");
        std::env::set_var("MAILRS_SUBMISSION_PORT", "0");
        std::env::set_var("MAILRS_SMTPS_PORT", "0");
        std::env::set_var("MAILRS_HOSTNAME", "mx.example.test");
    }
    let receiver = mailrs_receiver::start().await;
    let mut smtp = Smtp::connect(receiver.submission.port()).await;
    smtp.cmd("EHLO app.example.org").await;
    let plain = base64::engine::general_purpose::STANDARD.encode(format!("\0{BOB}\0{PASSWORD}"));
    let r = smtp.cmd(&format!("AUTH PLAIN {plain}")).await;
    assert!(r.starts_with("235"), "{r}");
    let submitted = message("smtp1@golia.example", "by smtp");
    let r = smtp.send(BOB, &["someone@example.org"], &submitted).await;
    assert!(r.starts_with("250"), "{r}");

    let incoming = spool.join("incoming/new");
    let root = maildir.to_str().unwrap().to_string();
    let drain = |state: Arc<mailrs_fastcore::FastcoreState>| {
        let (incoming, root) = (incoming.clone(), root.clone());
        tokio::task::spawn_blocking(move || {
            mailrs_fastcore::spool_drain::drain_once(&incoming, &root, &state).0
        })
    };
    assert_eq!(drain(state.clone()).await.unwrap(), 1);

    // the same message sent from the web
    let web = message("web1@golia.example", "by web");
    client
        .file_sent(BOB, web.as_bytes(), &["someone@example.org".to_string()])
        .await
        .unwrap();

    let req = ListConversationsRequest {
        filter: ConversationFilter {
            folder: Some("Sent".into()),
            limit: 50,
            ..Default::default()
        },
    };
    let rows = client.list_conversations(BOB, &req).await.unwrap().items;
    let row = |tid: &str| {
        rows.iter()
            .find(|r| r.thread_id == tid)
            .unwrap_or_else(|| panic!("{tid} is not in Sent: {rows:?}"))
    };
    let (smtp_row, web_row) = (row("smtp1@golia.example"), row("web1@golia.example"));
    assert_eq!(smtp_row.sent_count, 1, "{smtp_row:?}");
    assert_eq!(sender_side(smtp_row), sender_side(web_row));

    let sent = client.list_sent_messages(BOB).await.unwrap();
    let ids: Vec<&str> = sent.items.iter().map(|m| m.message_id.as_str()).collect();
    assert!(ids.contains(&"smtp1@golia.example"), "{ids:?}");
    let sent_files = || {
        std::fs::read_dir(maildir.join("golia.example/bob/.Sent/cur"))
            .map(|d| d.count())
            .unwrap_or(0)
    };
    assert_eq!(sent_files(), 2, "one file each, read");

    // a client retrying the same submission
    let r = smtp.send(BOB, &["someone@example.org"], &submitted).await;
    assert!(r.starts_with("250"), "{r}");
    assert_eq!(drain(state.clone()).await.unwrap(), 1);
    assert_eq!(sent_files(), 2, "the retry filed a second copy");
    let rows = client.list_conversations(BOB, &req).await.unwrap().items;
    let again = rows
        .iter()
        .find(|r| r.thread_id == "smtp1@golia.example")
        .unwrap();
    assert_eq!(sender_side(again), sender_side(smtp_row));
}
