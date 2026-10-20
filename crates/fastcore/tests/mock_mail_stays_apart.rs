//! Mock mail end to end: SMTP into a real receiver, the spool drained by
//! the real drain, read back from the mock store. And the other half:
//! nothing a real mailbox or the antispam state owns is touched.
//!
//! One test, because the receiver and the mock store read their
//! configuration from the environment once per process.

mod mock_support;

use std::sync::Arc;

use kevy_client::Connection;
use mailrs_core_sidestate::families::mock;
use mock_support::{Smtp, all, files_under, keys, mail_files_under, spool_files};

const KEVY: &str = "mem://mock-mail-stays-apart";
const DOMAIN: &str = "mock.example.test";

const SIGNUP: &str = "From: GitHub <noreply@github.com>\r\n\
To: e2e.signup@mock.example.test\r\n\
Subject: [GitHub] Please verify your device\r\n\
\r\n\
Your verification code is 482913. It expires in 15 minutes.";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mock_mail_arrives_once_and_touches_nothing_real() {
    let tmp = tempfile::tempdir().unwrap();
    let spool = tmp.path().join("spool");
    let maildir = tmp.path().join("maildir");
    let mock_root = tmp.path().join("mock");
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
        std::env::set_var("MAILRS_MOCK_DOMAIN", DOMAIN);
        std::env::set_var("MAILRS_MOCK_ROOT", &mock_root);
        std::env::set_var("MAILRS_SPOOL_ROOT", &spool);
        std::env::set_var("MAILRS_MAILDIR", &maildir);
        std::env::set_var(
            "MAILRS_LOCAL_DOMAINS",
            format!("golia.example.test,{DOMAIN}"),
        );
        std::env::set_var("MAILRS_PORT", "0");
        std::env::set_var("MAILRS_SUBMISSION_PORT", "0");
        std::env::set_var("MAILRS_SMTPS_PORT", "0");
        std::env::set_var("MAILRS_GREYLIST_ENABLED", "1");
        std::env::set_var("MAILRS_HOSTNAME", "mx.example.test");
    }
    // Open the in-process kevy before the receiver does: its first opens
    // race, and two racing opens of one mem:// name can each get a store.
    let mut kv = Connection::connect(KEVY).unwrap();
    let receiver = mailrs_receiver::start().await;

    mock::create_namespace(&mut kv, "dev@golia.example.test", "e2e").unwrap();
    let bob = maildir.join("golia.example.test/bob");
    for sub in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(bob.join(sub)).unwrap();
    }

    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    let mut state = mailrs_fastcore::FastcoreState::new(mailbox);
    state.net_url = Some(KEVY.to_string());
    let state = Arc::new(state);
    let incoming = spool.join("incoming/new");
    let drain =
        || mailrs_fastcore::spool_drain::drain_once(&incoming, maildir.to_str().unwrap(), &state);

    // 1. a signup code to a mock address
    let mut smtp = Smtp::connect(receiver.smtp.port()).await;
    smtp.cmd("EHLO sender.example.org").await;
    let before = keys(&mut kv);
    let r = smtp
        .send(
            "noreply@github.com",
            &["e2e.signup@mock.example.test"],
            SIGNUP,
        )
        .await;
    assert!(r.starts_with("250"), "{r}");
    let added: Vec<String> = keys(&mut kv).difference(&before).cloned().collect();
    assert!(
        added
            .iter()
            .all(|k| k.starts_with("mailrs:mock:") || k.starts_with("rate")),
        "the receiver wrote antispam state for mock-only mail: {added:?}"
    );

    let spooled = spool_files(&incoming);
    assert_eq!(spooled.len(), 1);
    let kept = std::fs::read(&spooled[0]).unwrap();
    assert_eq!(drain().0, 1);
    let got = all(&mut kv, "e2e");
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].tag, "signup");
    assert_eq!(got[0].summary.codes, ["482913"]);
    assert_eq!(
        files_under(&maildir),
        0,
        "a real mailbox received mock mail"
    );

    // 2. the same spool file drained again stores no second copy
    std::fs::write(&spooled[0], &kept).unwrap();
    drain();
    assert_eq!(all(&mut kv, "e2e").len(), 1);

    // 3. only mock recipients, none of whose namespaces exist
    let r = smtp
        .send(
            "a@example.org",
            &["nobody.x@mock.example.test"],
            "Subject: x\r\n\r\nx",
        )
        .await;
    assert!(r.starts_with("550"), "{r}");
    assert!(spool_files(&incoming).is_empty());

    // 4. a real and a mock recipient in one file: each gets its own copy
    let env = mailrs_core::spool::SpoolEnvelope {
        reverse_path: "a@example.org".into(),
        forward_paths: vec![
            "bob@golia.example.test".into(),
            "e2e.cc@mock.example.test".into(),
        ],
        is_authenticated: false,
        conn_id: 9,
        target_folder: "INBOX".into(),
        received_at: 1,
        fraud_verdict: None,
        sandbox_ns: None,
        schema_version: mailrs_core::spool::SPOOL_SCHEMA_VERSION,
    };
    let blob = mailrs_core::spool::encode_spool_blob(&env, b"Subject: both\r\n\r\nhello");
    std::fs::write(incoming.join("1700000000.M9P9.mixed"), blob).unwrap();
    assert_eq!(drain().0, 1);
    assert_eq!(
        mail_files_under(&bob),
        1,
        "the real recipient lost its copy"
    );
    assert!(all(&mut kv, "e2e").iter().any(|m| m.tag == "cc"));

    // 5. a sandbox login: everything is captured, nothing is queued outward
    let token = mock::rotate_smtp_token(&mut kv, "e2e").unwrap();
    let mut sub = Smtp::connect(receiver.submission.port()).await;
    sub.cmd("EHLO app.example.org").await;
    let plain = {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(format!("\0e2e@{DOMAIN}\0{token}"))
    };
    let wrong = {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(format!("\0e2e@{DOMAIN}\0nope"))
    };
    assert!(
        sub.cmd(&format!("AUTH PLAIN {wrong}"))
            .await
            .starts_with("535")
    );
    let r = sub.cmd(&format!("AUTH PLAIN {plain}")).await;
    assert!(r.starts_with("235"), "{r}");
    let before = keys(&mut kv);
    let r = sub
        .send(
            &format!("e2e@{DOMAIN}"),
            &["customer@example.org", "e2e.inbox@mock.example.test"],
            "Subject: welcome\r\n\r\nhi",
        )
        .await;
    assert!(r.starts_with("250"), "{r}");
    drain();
    let added: Vec<String> = keys(&mut kv).difference(&before).cloned().collect();
    assert!(
        !added.iter().any(|k| k.contains("outbound")),
        "a sandbox message was queued outward: {added:?}"
    );
    let msgs = all(&mut kv, "e2e");
    let captured: Vec<_> = msgs
        .iter()
        .filter(|m| m.direction == mock::Direction::CapturedOut)
        .collect();
    assert_eq!(captured.len(), 1);
    assert!(captured[0].rcpt.contains("customer@example.org"));
    assert!(
        msgs.iter()
            .any(|m| m.tag == "inbox" && m.direction == mock::Direction::In)
    );
    assert_eq!(mail_files_under(&bob), 1);
    assert!(spool_files(&incoming).is_empty());

    // 6. a simulation address answers a signed-in sender in their INBOX,
    //    through the bounce hand-off queue, and nobody from outside
    let pending = |kv: &mut Connection| kv.llen(mailrs_fastcore::bounce::BOUNCE_PENDING).unwrap();
    let spool = |name: &str, from: &str, authed: bool, to: &str| {
        let env = mailrs_core::spool::SpoolEnvelope {
            reverse_path: from.into(),
            forward_paths: vec![to.into()],
            is_authenticated: authed,
            conn_id: 10,
            target_folder: "INBOX".into(),
            received_at: 1,
            fraud_verdict: None,
            sandbox_ns: None,
            schema_version: mailrs_core::spool::SPOOL_SCHEMA_VERSION,
        };
        let raw = b"Subject: try\r\nMessage-ID: <t1@golia.example.test>\r\n\r\nhi";
        std::fs::write(
            incoming.join(name),
            mailrs_core::spool::encode_spool_blob(&env, raw),
        )
        .unwrap();
    };
    spool(
        "1700000001.M1.sim",
        "bob@golia.example.test",
        true,
        &format!("bounce@{DOMAIN}"),
    );
    assert_eq!(drain().0, 1);
    assert_eq!(
        pending(&mut kv),
        1,
        "the bounce was not handed to the sender's INBOX"
    );
    let id = kv
        .lrange(mailrs_fastcore::bounce::BOUNCE_PENDING, 0, 0)
        .unwrap()
        .remove(0);
    let key = format!("mailrs:bounce:{}", String::from_utf8(id).unwrap());
    assert_eq!(
        kv.hget(key.as_bytes(), b"recipient").unwrap().unwrap(),
        b"bob@golia.example.test"
    );
    spool(
        "1700000002.M1.sim",
        "stranger@example.org",
        false,
        &format!("complaint@{DOMAIN}"),
    );
    assert_eq!(drain().0, 1);
    assert_eq!(pending(&mut kv), 1, "an outside sender was answered");
    assert!(
        !keys(&mut kv).iter().any(|k| k.contains("suppress")),
        "a simulated bounce reached the suppression list"
    );

    // 7. a sandbox login's mail to ooto@ comes back into the namespace
    let r = sub
        .send(
            &format!("e2e@{DOMAIN}"),
            &[&format!("ooto@{DOMAIN}")],
            "Subject: hello\r\n\r\nhi",
        )
        .await;
    assert!(r.starts_with("250"), "{r}");
    drain();
    let ooto = all(&mut kv, "e2e");
    let reply = ooto
        .iter()
        .find(|m| m.tag == "ooto")
        .expect("no auto-reply in the namespace");
    assert_eq!(reply.direction, mock::Direction::In);
    assert!(reply.summary.subject.starts_with("Out of office"));
}
