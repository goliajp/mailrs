//! SMTP AUTH on a real receiver against a real fastcore takes no password
//! for an account without a usable one: an empty hash, which is what
//! clearing a password leaves, or a value an old import stored in clear,
//! which is never compared as text. An ordinary account still signs in, so
//! the refusals are about the password and not about AUTH itself.
//!
//! One test, because the receiver reads its configuration from the
//! environment once per process.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use mailrs_core_api::method::admin::{AddAccountRequest, SetPasswordRequest};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

const KEVY: &str = "mem://smtp-auth-takes-no-unusable-password";
const SECRET: &str = "secret";
const CLEAR_VALUE: &str = "clear-text-1";

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
            if line.is_empty() {
                return "<closed>".into();
            }
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
}

fn b64(s: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(s)
}

async fn auth_plain(port: u16, user: &str, password: &str) -> String {
    let mut smtp = Smtp::connect(port).await;
    smtp.last(&format!(
        "AUTH PLAIN {}",
        b64(&format!("\0{user}\0{password}"))
    ))
    .await
}

async fn auth_login(port: u16, user: &str, password: &str) -> String {
    let mut smtp = Smtp::connect(port).await;
    let r = smtp.last("AUTH LOGIN").await;
    assert!(r.starts_with("334"), "AUTH LOGIN: {r}");
    let r = smtp.last(&b64(user)).await;
    assert!(r.starts_with("334"), "AUTH LOGIN user: {r}");
    smtp.last(&b64(password)).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_password_signs_into_an_account_without_a_usable_one() {
    let tmp = tempfile::tempdir().unwrap();
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    let router = mailrs_fastcore::build_router_with_secret(
        Arc::new(mailrs_fastcore::FastcoreState::new(mailbox)),
        SECRET.into(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let core = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await });
    let client = mailrs_core_api::client::Client::new(core.clone(), SECRET);
    for (address, stored) in [
        ("bob@golia.example", None),
        ("blank@golia.example", Some("")),
        ("clear@golia.example", Some(CLEAR_VALUE)),
    ] {
        client
            .add_account(&AddAccountRequest {
                address: address.into(),
                display_name: String::new(),
                password: "a-real-password-1".into(),
                password_hash: None,
            })
            .await
            .unwrap();
        if let Some(stored) = stored {
            client
                .set_account_password(
                    address,
                    &SetPasswordRequest {
                        password_hash: stored.into(),
                    },
                )
                .await
                .unwrap();
        }
    }

    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
        std::env::set_var("MAILRS_CORE_RPC_BASE", &core);
        std::env::set_var("MAILRS_CORE_API_SECRET", SECRET);
        std::env::set_var("MAILRS_SPOOL_ROOT", tmp.path().join("spool"));
        std::env::set_var("MAILRS_MAILDIR", tmp.path().join("maildir"));
        std::env::set_var("MAILRS_LOCAL_DOMAINS", "golia.example");
        std::env::set_var("MAILRS_ANTISPAM_ENABLED", "false");
        std::env::set_var("MAILRS_PORT", "0");
        std::env::set_var("MAILRS_SUBMISSION_PORT", "0");
        std::env::set_var("MAILRS_SMTPS_PORT", "0");
        std::env::set_var("MAILRS_HOSTNAME", "mx.example.test");
    }
    let _kevy = kevy_client::Connection::connect(KEVY).unwrap();
    let receiver = mailrs_receiver::start().await;
    let port = receiver.submission.port();

    let r = auth_plain(port, "bob@golia.example", "a-real-password-1").await;
    assert!(r.starts_with("235"), "an ordinary account: {r}");

    let mut let_in = Vec::new();
    for (user, password) in [
        ("blank@golia.example", ""),
        ("blank@golia.example", "anything"),
        ("clear@golia.example", CLEAR_VALUE),
        ("clear@golia.example", ""),
    ] {
        let r = auth_plain(port, user, password).await;
        if r.starts_with("235") {
            let_in.push(format!("AUTH PLAIN {user} {password:?}: {r}"));
        }
    }
    let r = auth_login(port, "clear@golia.example", CLEAR_VALUE).await;
    if r.starts_with("235") {
        let_in.push(format!("AUTH LOGIN clear {CLEAR_VALUE:?}: {r}"));
    }
    assert!(let_in.is_empty(), "let in:\n{}", let_in.join("\n"));
}
