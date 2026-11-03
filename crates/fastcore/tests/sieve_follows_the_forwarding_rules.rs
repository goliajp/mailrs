//! A Sieve script's redirect and vacation reply, held to the rules the web
//! settings keep: no redirect out of our domains but to the verified
//! forwarding address, no reply as somebody the user may not send as, and
//! none to Junk. The scripts are written to the store directly, as one
//! stored before these rules or by an administrator would be. Spooled mail
//! drained by fastcore against a real kevy-server, the jobs read back off
//! the outbound queue.

use std::sync::Arc;

use base64::Engine as _;
use mailrs_core::spool::{SPOOL_SCHEMA_VERSION, SpoolEnvelope, encode_spool_blob};
use mailrs_core_sidestate::families::forwarding;
use mailrs_core_sidestate::families::outbound::{PENDING_IDX, job_key};
use mailrs_receiver::smtp_session::SpoolSink;
use mailrs_receiver::smtp_session::spool_sink::MaildirSpoolSink;
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};

const DAVE: &str = "dave@golia.example";
const ERIN: &str = "erin@golia.example";
const FAY: &str = "fay@golia.example";
const BOSS: &str = "boss@golia.example";
const FAY_AWAY: &str = "fay@elsewhere.example";

fn mail(id: &str, to: &str) -> Vec<u8> {
    format!(
        "From: Ann <ann@sender.example>\r\nTo: {to}\r\nSubject: hello {id}\r\n\
         Message-ID: <{id}@sender.example>\r\nDate: Tue, 5 Aug 2025 09:00:00 +0900\r\n\r\nhi\r\n"
    )
    .into_bytes()
}

fn envelope(to: &str, folder: &str) -> SpoolEnvelope {
    SpoolEnvelope {
        reverse_path: "ann@sender.example".into(),
        forward_paths: vec![to.into()],
        is_authenticated: false,
        auth_user: None,
        conn_id: 1,
        target_folder: folder.into(),
        received_at: 1_790_000_000,
        fraud_verdict: None,
        sandbox_ns: None,
        schema_version: SPOOL_SCHEMA_VERSION,
    }
}

/// (recipient, the message's From header)
fn queued(url: &str) -> Vec<(String, String)> {
    let mut c = kevy_client::Connection::connect(url).unwrap();
    let ids = c.lrange(PENDING_IDX, 0, -1).unwrap();
    let mut jobs: Vec<(String, String)> = ids
        .into_iter()
        .map(|id| {
            let id = String::from_utf8(id).unwrap();
            let blob = c.hget(job_key(id).as_bytes(), b"blob").unwrap().unwrap();
            let job: serde_json::Value = serde_json::from_slice(&blob).unwrap();
            let body = base64::engine::general_purpose::STANDARD
                .decode(job["message_data_b64"].as_str().unwrap())
                .unwrap();
            let from = String::from_utf8_lossy(&body)
                .lines()
                .find_map(|l| l.strip_prefix("From: ").map(str::to_string))
                .unwrap_or_default();
            (job["recipient"].as_str().unwrap().to_string(), from)
        })
        .collect();
    jobs.sort();
    jobs
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_script_redirects_and_replies_only_where_the_web_settings_could() {
    let _startup = mailrs_test_docker::startup_lock().await;
    let container = GenericImage::new("ghcr.io/goliajp/kevy", "6.4.0")
        .with_wait_for(WaitFor::message_on_stderr("starting:"))
        .with_exposed_port(6379.tcp())
        .with_startup_timeout(mailrs_test_docker::STARTUP_TIMEOUT)
        .start()
        .await
        .expect("start kevy container");
    let host = container.get_host().await.unwrap().to_string();
    let url = format!(
        "kevy://{}:{}",
        mailrs_test_docker::ipv4_loopback(&host),
        container.get_host_port_ipv4(6379).await.unwrap()
    );
    drop(_startup);

    let tmp = tempfile::tempdir().unwrap();
    let maildir = tmp.path().join("maildir");
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", &url);
        std::env::set_var("MAILRS_SRS_SECRET", "test-srs-secret");
        std::env::set_var("MAILRS_MAILDIR", &maildir);
        std::env::set_var("MAILRS_LOCAL_DOMAINS", "golia.example");
    }
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    for user in [DAVE, ERIN, FAY, BOSS] {
        mailbox.upsert_account(user, "{}").unwrap();
        let local = user.split('@').next().unwrap();
        for sub in ["cur", "new", "tmp"] {
            std::fs::create_dir_all(maildir.join("golia.example").join(local).join(sub)).unwrap();
        }
    }
    let state = Arc::new(mailrs_fastcore::FastcoreState::new(mailbox));

    let url2 = url.clone();
    tokio::task::spawn_blocking(move || {
        let mut c = (0..50)
            .find_map(|_| {
                let c = kevy_client::Connection::connect(&url2).ok();
                if c.is_none() {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                c
            })
            .expect("kevy answers");
        let mut script = |user: &str, body: &str| {
            let key = mailrs_core_sidestate::sieve_key(user);
            c.set(key.as_bytes(), body.as_bytes()).unwrap();
        };
        script(DAVE, r#"redirect "thief@out.example";"#);
        script(
            ERIN,
            r#"require "vacation"; vacation :from "boss@golia.example" :days 1 "away";"#,
        );
        script(FAY, r#"redirect "fay@elsewhere.example";"#);
        let code = forwarding::request(&mut c, FAY, FAY_AWAY, true, 1).unwrap();
        forwarding::verify(&mut c, FAY, &code, 2).unwrap();
    })
    .await
    .unwrap();

    let spool = tmp.path().join("spool");
    let sink = MaildirSpoolSink::new(&spool.to_string_lossy());
    for (id, to, folder) in [
        ("d1", DAVE, "INBOX"),
        ("e1", ERIN, "INBOX"),
        ("e2", ERIN, "Junk"),
        ("f1", FAY, "INBOX"),
    ] {
        sink.write(&encode_spool_blob(&envelope(to, folder), &mail(id, to)))
            .await
            .unwrap();
    }
    let incoming = spool.join("incoming").join("new");
    let md = maildir.to_string_lossy().to_string();
    let st = state.clone();
    let drained = tokio::task::spawn_blocking(move || {
        mailrs_fastcore::spool_drain::drain_once(&incoming, &md, &st).0
    })
    .await
    .unwrap();
    assert_eq!(drained, 4);

    let jobs = tokio::task::spawn_blocking(move || queued(&url))
        .await
        .unwrap();
    // nothing to thief@; one reply to Ann, as Erin, for the INBOX letter
    // only; Fay's verified address forwarded
    assert_eq!(jobs.len(), 2, "{jobs:?}");
    assert_eq!(jobs[0].0, "ann@sender.example");
    assert_eq!(jobs[0].1, format!("<{ERIN}>"));
    assert_eq!(jobs[1].0, FAY_AWAY);

    // the refused redirect kept the letter here
    assert!(
        state
            .mailbox
            .get_thread_for_user(DAVE, "d1@sender.example")
            .unwrap()
            .is_some()
    );
}
