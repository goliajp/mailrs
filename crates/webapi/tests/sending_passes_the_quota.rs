//! A full mailbox still sends: the message is queued, its copy is filed
//! in Sent, and the usage goes past the quota. Only imports stop there.
//! The usage reads back past the quota, which is what the web banner
//! shows.
//!
//! Against a real kevy-server: the queue write is pipelined.

mod live;

use mailrs_core_sidestate::keys::quota::{limit_key, used_key};
use reqwest::Method;
use serde_json::json;

const ALICE: &str = "alice@example.test";
const LIMIT: i64 = 1000;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_full_mailbox_still_sends_and_files_its_copy() {
    let (_kevy, kevy) = mailrs_test_docker::kevy::kevy_server().await;
    let tmp = tempfile::tempdir().expect("tmp");
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", &kevy);
        std::env::set_var("MAILRS_MAILDIR", tmp.path());
    }
    let live = live::start(&kevy).await;
    live.add_account(ALICE, "pw").await;
    let alice = live.login(ALICE, "pw").await;
    let mut net = kevy_client::Connection::connect(&kevy).expect("kevy");
    net.set(limit_key(ALICE).as_bytes(), LIMIT.to_string().as_bytes())
        .expect("limit");
    net.set(used_key(ALICE).as_bytes(), LIMIT.to_string().as_bytes())
        .expect("used");

    let mail = json!({
        "from": ALICE, "to": ["bob@partner.test"], "subject": "still here", "body": "x",
    });
    let (status, got) = live
        .call(Method::POST, "/api/mail/send", &alice, Some(mail))
        .await;
    assert_eq!(status, 200, "a full mailbox could not send: {got}");

    let sent = tmp.path().join("example.test/alice/.Sent/cur");
    let filed = std::fs::read_dir(sent).map(|d| d.count()).unwrap_or(0);
    assert_eq!(filed, 1, "the copy is not in Sent");

    let used: i64 = String::from_utf8(
        net.get(used_key(ALICE).as_bytes())
            .expect("read")
            .expect("usage"),
    )
    .unwrap()
    .parse()
    .unwrap();
    assert!(used > LIMIT, "the usage did not count the copy: {used}");

    let (status, storage) = live.call(Method::GET, "/api/storage", &alice, None).await;
    assert_eq!(status, 200, "{storage}");
    assert_eq!(storage["quota_bytes"], LIMIT);
    assert_eq!(storage["used_bytes"], used);
}
