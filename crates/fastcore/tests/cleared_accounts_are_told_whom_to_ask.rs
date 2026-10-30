//! A sign-in to an account whose password an administrator cleared is told
//! so, and whom to ask for a reset code, on IMAP, POP3, ManageSieve and SMTP
//! submission. Only after a password was tried, and with the same answer
//! for any password: an account that was never cleared, and an address with
//! no account, are answered as before, so the notice tells nobody which
//! addresses exist beyond those being reset.
//!
//! One test, because the receiver reads its configuration from the
//! environment once per process.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use mailrs_core_api::method::admin::{AddAccountRequest, ClearPasswordRequest};
use mailrs_mail_protocols::Guarded;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

const KEVY: &str = "mem://cleared-accounts-are-told-whom-to-ask";
const SECRET: &str = "secret";
const CLEARED: &str = "clara@golia.example";
const KEPT: &str = "kept@golia.example";
const NOTICE: &str = "Your password was reset by an administrator. Ask Li Hao <lihao@golia.example> for a reset code to set a new one";

async fn line<R: AsyncRead + Unpin>(r: &mut BufReader<R>) -> String {
    let mut l = String::new();
    match tokio::time::timeout(Duration::from_secs(5), r.read_line(&mut l)).await {
        Ok(Ok(0)) => "<closed>".into(),
        Ok(Ok(_)) => l.trim_end().to_string(),
        _ => "<nothing>".into(),
    }
}

type Dir = Arc<Guarded<mailrs_fastcore::FastcoreDirectory>>;

/// The greeting read, `send` written, the reply's last line returned.
async fn exchange(client: tokio::io::DuplexStream, greeting_lines: usize, send: &str) -> String {
    let (r, mut w) = tokio::io::split(client);
    let mut r = BufReader::new(r);
    for _ in 0..greeting_lines {
        line(&mut r).await;
    }
    w.write_all(send.as_bytes()).await.unwrap();
    let mut last = line(&mut r).await;
    // POP3 answers USER before PASS
    if last == "+OK send PASS" {
        last = line(&mut r).await;
    }
    last
}

async fn imap(dir: &Dir, who: &str, pw: &str) -> String {
    let (client, server) = tokio::io::duplex(1 << 16);
    tokio::spawn(mailrs_mail_protocols::imap::run(dir.clone(), server));
    exchange(client, 1, &format!("a1 LOGIN {who} \"{pw}\"\r\n")).await
}

async fn pop3(dir: &Dir, who: &str, pw: &str) -> String {
    let (client, server) = tokio::io::duplex(1 << 16);
    tokio::spawn(mailrs_mail_protocols::pop3::session(
        dir.clone(),
        server,
        None,
    ));
    exchange(client, 1, &format!("USER {who}\r\nPASS {pw}\r\n")).await
}

async fn sieve(dir: &Dir, who: &str, pw: &str) -> String {
    let (client, server) = tokio::io::duplex(1 << 16);
    tokio::spawn(mailrs_mail_protocols::managesieve::session(
        dir.clone(),
        server,
        None,
    ));
    let plain = base64::engine::general_purpose::STANDARD.encode(format!("\0{who}\0{pw}"));
    exchange(
        client,
        5,
        &format!("AUTHENTICATE \"PLAIN\" \"{plain}\"\r\n"),
    )
    .await
}

fn expect(wrong: &mut Vec<String>, what: &str, got: String, want: &str) {
    if got != want {
        wrong.push(format!("{what}\n    got:  {got}\n    want: {want}"));
    }
}

async fn smtp(port: u16, user: &str, password: &str) -> String {
    let (r, mut w) = TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap()
        .into_split();
    let mut r = BufReader::new(r);
    line(&mut r).await;
    w.write_all(b"EHLO sender.example.org\r\n").await.unwrap();
    while !line(&mut r).await.starts_with("250 ") {}
    let plain = base64::engine::general_purpose::STANDARD.encode(format!("\0{user}\0{password}"));
    w.write_all(format!("AUTH PLAIN {plain}\r\n").as_bytes())
        .await
        .unwrap();
    line(&mut r).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cleared_account_is_told_whom_to_ask_and_nobody_else_is() {
    let tmp = tempfile::tempdir().unwrap();
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    let state = Arc::new(mailrs_fastcore::FastcoreState::new(mailbox));
    let router = mailrs_fastcore::build_router_with_secret(state.clone(), SECRET.into());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let core = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await });
    let client = mailrs_core_api::client::Client::new(core.clone(), SECRET);
    for address in [CLEARED, KEPT] {
        client
            .add_account(&AddAccountRequest {
                address: address.into(),
                display_name: String::new(),
                password: "a-real-password-1".into(),
                password_hash: None,
            })
            .await
            .unwrap();
    }
    client
        .clear_account_password(
            CLEARED,
            &ClearPasswordRequest {
                by: "root@golia.example".into(),
            },
        )
        .await
        .unwrap();

    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_RESET_CONTACT_NAME", "Li Hao");
        std::env::set_var("MAILRS_RESET_CONTACT", "lihao@golia.example");
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
    let dir = mailrs_fastcore::FastcoreDirectory::new(state, tmp.path().join("maildir"));
    let guard = mailrs_auth_guard::kevy::guard_for(Some(KEVY));
    let dir = Arc::new(Guarded::new(Arc::new(dir), guard));
    let receiver = mailrs_receiver::start().await;
    let port = receiver.submission.port();

    let mut wrong = Vec::new();
    for (who, notice) in [
        (CLEARED, true),
        (KEPT, false),
        ("nobody@golia.example", false),
    ] {
        let pw = "any-password-1";
        let (i, p, m) = match notice {
            true => (
                format!("a1 NO [EXPIRED] {NOTICE}"),
                format!("-ERR [AUTH] {NOTICE}"),
                format!("NO \"{NOTICE}\""),
            ),
            false => (
                "a1 NO invalid credentials".to_string(),
                "-ERR invalid credentials".to_string(),
                "NO \"Authentication failed\"".to_string(),
            ),
        };
        expect(
            &mut wrong,
            &format!("imap {who}"),
            imap(&dir, who, pw).await,
            &i,
        );
        expect(
            &mut wrong,
            &format!("pop3 {who}"),
            pop3(&dir, who, pw).await,
            &p,
        );
        expect(
            &mut wrong,
            &format!("managesieve {who}"),
            sieve(&dir, who, pw).await,
            &m,
        );
        let smtp_reply = smtp(port, who, pw).await;
        match notice {
            true => expect(
                &mut wrong,
                &format!("smtp {who}"),
                smtp_reply,
                &format!("535 5.7.8 {NOTICE}"),
            ),
            false if smtp_reply.contains("reset") || !smtp_reply.starts_with("535") => {
                wrong.push(format!("smtp {who}: {smtp_reply}"));
            }
            false => {}
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
