//! A real receiver, started the way production starts it, against a real
//! fastcore: a domain on the Domains page is received, any other is
//! refused as relaying, and the size limit saved on the System Config page
//! is what EHLO advertises and what DATA enforces.
//!
//! One test, because the receiver reads its configuration from the
//! environment once per process.

use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

const KEVY: &str = "mem://receiver-reads-the-admin-pages";
const LIMIT: usize = 2048;

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
        for _ in 0..200 {
            if let Ok(s) = TcpStream::connect(("127.0.0.1", port)).await {
                let (r, w) = s.into_split();
                let mut smtp = Self {
                    reader: BufReader::new(r),
                    writer: w,
                };
                smtp.reply().await;
                return smtp;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("receiver never listened");
    }

    // every line of one reply
    async fn reply(&mut self) -> Vec<String> {
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            self.reader.read_line(&mut line).await.unwrap();
            assert!(!line.is_empty(), "connection closed after {lines:?}");
            let last = line.as_bytes().get(3) != Some(&b'-');
            lines.push(line.trim_end().to_string());
            if last {
                return lines;
            }
        }
    }

    async fn cmd(&mut self, line: &str) -> Vec<String> {
        self.writer
            .write_all(format!("{line}\r\n").as_bytes())
            .await
            .unwrap();
        self.reply().await
    }

    async fn last(&mut self, line: &str) -> String {
        self.cmd(line).await.pop().unwrap()
    }

    async fn send(&mut self, to: &str, body: &str) -> String {
        assert!(
            self.last("MAIL FROM:<a@example.org>")
                .await
                .starts_with("250")
        );
        assert!(
            self.last(&format!("RCPT TO:<{to}>"))
                .await
                .starts_with("250")
        );
        assert!(self.last("DATA").await.starts_with("354"));
        self.last(&format!("{body}\r\n.")).await
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_receiver_takes_store_domains_and_the_stored_size_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let smtp_port = free_port();

    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    mailbox.upsert_domain("added.example", 1).unwrap();
    // RCPT to a local address nobody receives is refused
    for bob in ["bob@added.example", "bob@golia.example"] {
        mailbox.upsert_account(bob, "{}").unwrap();
    }
    let router =
        mailrs_fastcore::build_router(Arc::new(mailrs_fastcore::FastcoreState::new(mailbox)));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let core = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await });

    let mut kv = kevy_client::Connection::connect(KEVY).unwrap();
    kv.hset(
        b"admin:system-config",
        &[(&b"max_message_size_bytes"[..], LIMIT.to_string().as_bytes())],
    )
    .unwrap();

    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
        std::env::set_var("MAILRS_CORE_RPC_BASE", &core);
        std::env::set_var("MAILRS_CORE_API_SECRET", "secret");
        std::env::set_var("MAILRS_SPOOL_ROOT", tmp.path().join("spool"));
        std::env::set_var("MAILRS_MAILDIR", tmp.path().join("maildir"));
        std::env::set_var("MAILRS_LOCAL_DOMAINS", "golia.example");
        std::env::set_var("MAILRS_ANTISPAM_ENABLED", "false");
        std::env::set_var("MAILRS_PORT", smtp_port.to_string());
        std::env::set_var("MAILRS_SUBMISSION_PORT", free_port().to_string());
        std::env::set_var("MAILRS_SMTPS_PORT", free_port().to_string());
        std::env::set_var("MAILRS_HOSTNAME", "mx.example.test");
    }
    tokio::spawn(mailrs_receiver::run());

    let mut smtp = Smtp::connect(smtp_port).await;
    let ehlo = smtp.cmd("EHLO sender.example.org").await;
    assert!(
        ehlo.iter().any(|l| l.ends_with(&format!("SIZE {LIMIT}"))),
        "{ehlo:?}"
    );

    // the domain on the page is ours, the environment's still is, and a
    // third is relaying
    assert!(
        smtp.send("bob@added.example", "Subject: a\r\n\r\na")
            .await
            .starts_with("250")
    );
    assert!(
        smtp.send("bob@golia.example", "Subject: b\r\n\r\nb")
            .await
            .starts_with("250")
    );
    let r = smtp
        .send("bob@elsewhere.example", "Subject: c\r\n\r\nc")
        .await;
    assert!(r.starts_with("550"), "{r}");

    // a declared size over the limit is refused at MAIL FROM
    let r = smtp
        .last(&format!("MAIL FROM:<a@example.org> SIZE={}", LIMIT * 2))
        .await;
    assert!(r.starts_with("552"), "{r}");

    // an undeclared one is refused when DATA passes the limit
    let big = format!("Subject: big\r\n\r\n{}", "x".repeat(LIMIT * 2));
    let r = smtp.send("bob@added.example", &big).await;
    assert!(r.starts_with("552"), "{r}");
}
