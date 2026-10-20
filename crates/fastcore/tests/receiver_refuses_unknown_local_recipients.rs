//! A real receiver against a real fastcore: RCPT to a local address nobody
//! receives is refused with 550 5.1.1, signed-in submission included; an
//! account is taken; a remote recipient is still refused as relaying at
//! DATA; and with fastcore down every local recipient is accepted as before.
//!
//! One test, because the receiver reads its configuration from the
//! environment once per process.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

const KEVY: &str = "mem://receiver-refuses-unknown-local-recipients";

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
                smtp.last_line().await;
                smtp.last("EHLO sender.example.org").await;
                return smtp;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("receiver never listened");
    }

    async fn last_line(&mut self) -> String {
        loop {
            let mut line = String::new();
            self.reader.read_line(&mut line).await.unwrap();
            assert!(!line.is_empty(), "connection closed");
            if line.as_bytes().get(3) != Some(&b'-') {
                return line.trim_end().to_string();
            }
        }
    }

    async fn last(&mut self, line: &str) -> String {
        self.writer
            .write_all(format!("{line}\r\n").as_bytes())
            .await
            .unwrap();
        self.last_line().await
    }

    async fn rcpt(&mut self, to: &str) -> String {
        let r = self.last("MAIL FROM:<a@example.org>").await;
        assert!(r.starts_with("250"), "{r}");
        let r = self.last(&format!("RCPT TO:<{to}>")).await;
        assert!(self.last("RSET").await.starts_with("250"));
        r
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unknown_local_recipients_are_refused_at_rcpt() {
    let tmp = tempfile::tempdir().unwrap();

    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    let router =
        mailrs_fastcore::build_router(Arc::new(mailrs_fastcore::FastcoreState::new(mailbox)));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let core = format!("http://{}", listener.local_addr().unwrap());
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let fastcore = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = stopped.await;
            })
            .await
    });
    let client = mailrs_core_api::client::Client::new(core.clone(), "secret");
    client
        .add_account(&mailrs_core_api::method::admin::AddAccountRequest {
            address: "bob@golia.example".into(),
            display_name: "Bob".into(),
            password: "pw".into(),
        })
        .await
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
        std::env::set_var("MAILRS_PORT", "0");
        std::env::set_var("MAILRS_SUBMISSION_PORT", "0");
        std::env::set_var("MAILRS_SMTPS_PORT", "0");
        std::env::set_var("MAILRS_HOSTNAME", "mx.example.test");
    }
    // Hold the in-process kevy open before the receiver opens it: its first
    // opens race, and two racing opens of one mem:// name can each get a store.
    let _kevy = kevy_client::Connection::connect(KEVY).unwrap();
    let receiver = mailrs_receiver::start().await;

    let mut smtp = Smtp::connect(receiver.smtp.port()).await;
    let r = smtp.rcpt("nobody@golia.example").await;
    assert_eq!(r, "550 5.1.1 <nobody@golia.example>: no such user");
    let r = smtp.rcpt("bob@golia.example").await;
    assert!(r.starts_with("250"), "{r}");

    // a refused RCPT leaves the transaction as it was
    assert!(
        smtp.last("MAIL FROM:<a@example.org>")
            .await
            .starts_with("250")
    );
    assert!(
        smtp.last("RCPT TO:<nobody@golia.example>")
            .await
            .starts_with("550")
    );
    assert!(
        smtp.last("RCPT TO:<bob@golia.example>")
            .await
            .starts_with("250")
    );
    assert!(smtp.last("DATA").await.starts_with("354"));
    assert!(
        smtp.last("Subject: x\r\n\r\nx\r\n.")
            .await
            .starts_with("250")
    );

    // a remote recipient is not asked about, and is refused as relaying
    assert!(
        smtp.last("MAIL FROM:<a@example.org>")
            .await
            .starts_with("250")
    );
    assert!(
        smtp.last("RCPT TO:<carol@elsewhere.example>")
            .await
            .starts_with("250")
    );
    assert!(smtp.last("DATA").await.starts_with("354"));
    let r = smtp.last("Subject: y\r\n\r\ny\r\n.").await;
    assert!(r.starts_with("550 5.7.1"), "{r}");

    // a signed-in sender gets the same answer for a local address
    let mut sub = Smtp::connect(receiver.submission.port()).await;
    let plain = base64::engine::general_purpose::STANDARD.encode(b"\0bob@golia.example\0pw");
    let r = sub.last(&format!("AUTH PLAIN {plain}")).await;
    assert!(r.starts_with("235"), "{r}");
    let r = sub.rcpt("nobody@golia.example").await;
    assert!(r.starts_with("550 5.1.1"), "{r}");

    // fastcore down: accepted as before
    stop.send(()).unwrap();
    fastcore.await.unwrap().unwrap();
    let r = smtp.rcpt("nobody@golia.example").await;
    assert!(r.starts_with("250"), "{r}");
}
