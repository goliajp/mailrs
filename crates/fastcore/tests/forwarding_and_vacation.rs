//! Forwarding and the vacation reply set on the web, end to end: spooled
//! mail drained by fastcore, the jobs read back off the outbound queue the
//! sender claims from. Against a real kevy-server, because the queue is
//! written with a pipeline.

use std::sync::Arc;

use mailrs_core::spool::{SPOOL_SCHEMA_VERSION, SpoolEnvelope, encode_spool_blob};
use mailrs_core_sidestate::families::outbound::{PENDING_IDX, job_key};
use mailrs_core_sidestate::families::{forwarding, vacation};
use mailrs_receiver::smtp_session::SpoolSink;
use mailrs_receiver::smtp_session::spool_sink::MaildirSpoolSink;
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};

const BOB: &str = "bob@golia.example";
const CAROL: &str = "carol@golia.example";
const AWAY: &str = "bob@elsewhere.example";

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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn forwarding_and_the_vacation_reply_act_on_arriving_mail() {
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
    for user in [BOB, CAROL] {
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
        let code = forwarding::request(&mut c, BOB, AWAY, false, 1).unwrap();
        assert!(matches!(
            forwarding::verify(&mut c, BOB, &code, 2).unwrap(),
            forwarding::Verified::On(_)
        ));
        let away = vacation::Vacation {
            enabled: true,
            subject: "Away".into(),
            body: "Back on Monday.".into(),
            start: None,
            end: None,
            days: 7,
            version: 0,
        };
        vacation::set(&mut c, CAROL, away).unwrap().unwrap();
    })
    .await
    .unwrap();

    let spool = tmp.path().join("spool");
    let sink = MaildirSpoolSink::new(&spool.to_string_lossy());
    for (id, to, folder) in [
        ("f1", BOB, "INBOX"),
        ("f2", BOB, "Junk"),
        ("v1", CAROL, "INBOX"),
        ("v2", CAROL, "INBOX"),
        ("v3", CAROL, "Junk"),
    ] {
        sink.write(&encode_spool_blob(&envelope(to, folder), &mail(id, to)))
            .await
            .unwrap();
    }
    // forwarded from bob once already, and back: round a loop
    let looped = [
        format!("Delivered-To: {BOB}\r\n").into_bytes(),
        mail("f3", BOB),
    ]
    .concat();
    sink.write(&encode_spool_blob(&envelope(BOB, "INBOX"), &looped))
        .await
        .unwrap();
    let incoming = spool.join("incoming").join("new");
    let md = maildir.to_string_lossy().to_string();
    let st = state.clone();
    let drained = tokio::task::spawn_blocking(move || {
        mailrs_fastcore::spool_drain::drain_once(&incoming, &md, &st).0
    })
    .await
    .unwrap();
    assert_eq!(drained, 6);

    let jobs = tokio::task::spawn_blocking(move || {
        let mut c = kevy_client::Connection::connect(&url).unwrap();
        let ids = c.lrange(PENDING_IDX, 0, -1).unwrap();
        let mut jobs: Vec<(String, String, Vec<u8>)> = ids
            .into_iter()
            .map(|id| {
                use base64::Engine as _;
                let id = String::from_utf8(id).unwrap();
                let blob = c.hget(job_key(id).as_bytes(), b"blob").unwrap().unwrap();
                let job: serde_json::Value = serde_json::from_slice(&blob).unwrap();
                (
                    job["recipient"].as_str().unwrap().to_string(),
                    job["sender"].as_str().unwrap().to_string(),
                    base64::engine::general_purpose::STANDARD
                        .decode(job["message_data_b64"].as_str().unwrap())
                        .unwrap(),
                )
            })
            .collect();
        jobs.sort();
        jobs
    })
    .await
    .unwrap();
    // one forward, with the sender rewritten onto our domain so SPF passes;
    // one vacation reply, null sender, once for two letters; none for Junk
    assert_eq!(jobs.len(), 2, "{jobs:?}");
    assert_eq!(jobs[0].0, "ann@sender.example");
    assert_eq!(jobs[0].1, "<>");
    assert_eq!(jobs[1].0, AWAY);
    assert!(jobs[1].1.starts_with("SRS0="), "{jobs:?}");
    assert!(jobs[1].1.ends_with("@golia.example"), "{jobs:?}");
    // the forward names the address it left from, so it cannot come back
    // round and go out again
    let forwarded = String::from_utf8_lossy(&jobs[1].2);
    assert!(
        forwarded.starts_with(&format!("Delivered-To: {BOB}\r\n")),
        "{forwarded}"
    );

    // forwarded without a copy here; Junk was not forwarded and stays
    let m = &state.mailbox;
    assert!(
        m.get_thread_for_user(BOB, "f1@sender.example")
            .unwrap()
            .is_none()
    );
    assert!(
        m.get_thread_for_user(BOB, "f2@sender.example")
            .unwrap()
            .is_some()
    );
    assert!(
        m.get_thread_for_user(BOB, "f3@sender.example")
            .unwrap()
            .is_some(),
        "the looped copy is not kept here"
    );
    assert!(
        m.get_thread_for_user(CAROL, "v1@sender.example")
            .unwrap()
            .is_some()
    );
}
