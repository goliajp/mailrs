//! A message submitted over SMTP AUTH from one of the account's send-as
//! addresses leaves on an outbound job that names the signed-in account,
//! so the sender counts and pauses the account, not the address. Mail
//! submitted from the account's own address names it too, so the job does
//! not depend on which address was used.
//!
//! One test, because the receiver reads its configuration from the
//! environment once per process. Against a kevy-server: writing a job
//! pipelines, which the in-process store does not.

// shared with the mock-namespace tests; this one needs only its SMTP client
#[allow(dead_code)]
mod mock_support;

use std::sync::Arc;

use base64::Engine;
use mailrs_core_api::method::admin::{AddAccountRequest, SendAsList};
use mailrs_core_sidestate::families::outbound as ob;
use mock_support::Smtp;

const SECRET: &str = "secret";
const BOB: &str = "bob@golia.example";
const SALES: &str = "sales@golia.example";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_submitted_job_names_the_account_that_signed_in() {
    let (_kevy, url) = mailrs_test_docker::kevy::kevy_server().await;
    let tmp = tempfile::tempdir().unwrap();
    let spool = tmp.path().join("spool");
    let maildir = tmp.path().join("maildir");
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    let mut state = mailrs_fastcore::FastcoreState::new(mailbox);
    state.net_url = Some(url.clone());
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
            password: "a-real-password-1".into(),
            password_hash: None,
        })
        .await
        .unwrap();
    client
        .set_send_as(
            BOB,
            &SendAsList {
                send_as: vec![SALES.into()],
            },
        )
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
    let plain =
        base64::engine::general_purpose::STANDARD.encode(format!("\0{}\0a-real-password-1", BOB));
    let r = smtp.cmd(&format!("AUTH PLAIN {plain}")).await;
    assert!(r.starts_with("235"), "{r}");
    for from in [SALES, BOB] {
        let body = format!("From: {from}\r\nSubject: hi\r\n\r\nhello");
        let r = smtp.send(from, &["someone@example.org"], &body).await;
        assert!(r.starts_with("250"), "{from}: {r}");
    }

    let incoming = spool.join("incoming/new");
    let (drained, _) =
        mailrs_fastcore::spool_drain::drain_once(&incoming, maildir.to_str().unwrap(), &state);
    assert_eq!(drained, 2);
    let mut kv = kevy_client::Connection::connect(&url).unwrap();
    let last: i64 = kv
        .get(ob::COUNTER)
        .unwrap()
        .map(|v| String::from_utf8(v).unwrap().parse().unwrap())
        .unwrap_or(0);
    let mut jobs = Vec::new();
    for id in 1..=last {
        let Some(blob) = kv.hget(ob::job_key(id).as_bytes(), b"blob").unwrap() else {
            continue;
        };
        let job: serde_json::Value = serde_json::from_slice(&blob).unwrap();
        jobs.push((
            job["sender"].as_str().unwrap().to_string(),
            job["account"].as_str().map(String::from),
        ));
    }
    jobs.sort();
    assert_eq!(
        jobs,
        [
            (BOB.to_string(), Some(BOB.to_string())),
            (SALES.to_string(), Some(BOB.to_string())),
        ]
    );
}
