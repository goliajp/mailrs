//! Mock mail end to end: SMTP into a real receiver, the spool drained by
//! the real drain, read back from the mock store. And the other half:
//! nothing a real mailbox or the antispam state owns is touched.
//!
//! One test, because the receiver and the mock store read their
//! configuration from the environment once per process.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use kevy_client::Connection;
use mailrs_core_sidestate::families::mock;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

const KEVY: &str = "mem://mock-mail-stays-apart";
const DOMAIN: &str = "mock.example.test";

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

struct Smtp {
    reader: BufReader<tokio::net::tcp::OwnedReadHalf>,
    writer: tokio::net::tcp::OwnedWriteHalf,
}

impl Smtp {
    async fn connect(port: u16) -> Self {
        let mut last = None;
        for _ in 0..100 {
            match TcpStream::connect(("127.0.0.1", port)).await {
                Ok(s) => {
                    let (r, w) = s.into_split();
                    let mut smtp = Self {
                        reader: BufReader::new(r),
                        writer: w,
                    };
                    smtp.reply().await;
                    return smtp;
                }
                Err(e) => last = Some(e),
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("receiver never listened: {last:?}");
    }

    // the final line of a reply, e.g. "250 OK"
    async fn reply(&mut self) -> String {
        loop {
            let mut line = String::new();
            self.reader.read_line(&mut line).await.unwrap();
            assert!(!line.is_empty(), "connection closed");
            if line.as_bytes().get(3) != Some(&b'-') {
                return line.trim_end().to_string();
            }
        }
    }

    async fn cmd(&mut self, line: &str) -> String {
        self.writer
            .write_all(format!("{line}\r\n").as_bytes())
            .await
            .unwrap();
        self.reply().await
    }

    /// One transaction; the reply to the end of DATA, or to the first
    /// command refused before it.
    async fn send(&mut self, from: &str, to: &[&str], body: &str) -> String {
        let r = self.cmd(&format!("MAIL FROM:<{from}>")).await;
        if !r.starts_with('2') {
            return r;
        }
        for rcpt in to {
            let r = self.cmd(&format!("RCPT TO:<{rcpt}>")).await;
            if !r.starts_with('2') {
                return r;
            }
        }
        assert!(self.cmd("DATA").await.starts_with("354"));
        self.cmd(&format!("{body}\r\n.")).await
    }
}

fn keys(c: &mut Connection) -> BTreeSet<String> {
    c.keys(b"*")
        .unwrap()
        .into_iter()
        .map(|k| String::from_utf8_lossy(&k).into_owned())
        .collect()
}

fn spool_files(dir: &Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

fn all(c: &mut Connection, ns: &str) -> Vec<mock::MessageMeta> {
    let f = mock::Filter {
        limit: usize::MAX,
        ..Default::default()
    };
    mock::list_messages(c, ns, &f).unwrap()
}

fn files_under(dir: &Path) -> usize {
    walk(dir).len()
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
    }
    out
}

const SIGNUP: &str = "From: GitHub <noreply@github.com>\r\n\
To: e2e.signup@mock.example.test\r\n\
Subject: [GitHub] Please verify your device\r\n\
\r\n\
Your verification code is 482913. It expires in 15 minutes.";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mock_mail_arrives_once_and_touches_nothing_real() {
    let tmp = tempfile::tempdir().unwrap();
    let (smtp_port, submission_port) = (free_port(), free_port());
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
        std::env::set_var("MAILRS_PORT", smtp_port.to_string());
        std::env::set_var("MAILRS_SUBMISSION_PORT", submission_port.to_string());
        std::env::set_var("MAILRS_SMTPS_PORT", free_port().to_string());
        std::env::set_var("MAILRS_GREYLIST_ENABLED", "1");
        std::env::set_var("MAILRS_HOSTNAME", "mx.example.test");
    }
    tokio::spawn(mailrs_receiver::run());

    let mut kv = Connection::connect(KEVY).unwrap();
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
    let mut smtp = Smtp::connect(smtp_port).await;
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
    assert_eq!(files_under(&bob), 1, "the real recipient lost its copy");
    assert!(all(&mut kv, "e2e").iter().any(|m| m.tag == "cc"));

    // 5. a sandbox login: everything is captured, nothing is queued outward
    let token = mock::rotate_smtp_token(&mut kv, "e2e").unwrap();
    let mut sub = Smtp::connect(submission_port).await;
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
    assert_eq!(files_under(&bob), 1);
    assert!(spool_files(&incoming).is_empty());
}
