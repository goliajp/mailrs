//! Ending an account's sign-ins closes the connections it already has open:
//! an IMAP client in IDLE, a POP3 session, and an SMTP submission session on
//! a real receiver, each told why. Another account's connection stays. The
//! sign-out travels the real channel to the real listeners, as webapi sends
//! it when a password is cleared.
//!
//! One test, because the receiver reads its configuration from the
//! environment once per process.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use mailrs_core_api::method::admin::AddAccountRequest;
use mailrs_core_sidestate::keys::signout;
use mailrs_mail_protocols::Guarded;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};
use tokio::net::TcpStream;

const KEVY: &str = "mem://signout-closes-signed-in-connections";
const SECRET: &str = "secret";
const ALICE: &str = "alice@golia.example";
const BOB: &str = "bob@golia.example";
const PW: &str = "a-real-password-1";

struct Conn<R> {
    reader: BufReader<R>,
}

impl<R: tokio::io::AsyncRead + Unpin> Conn<R> {
    async fn line(&mut self) -> String {
        let mut line = String::new();
        match tokio::time::timeout(Duration::from_secs(5), self.reader.read_line(&mut line)).await {
            Ok(Ok(0)) => "<closed>".into(),
            Ok(Ok(_)) => line.trim_end().to_string(),
            Ok(Err(e)) => format!("<error {e}>"),
            Err(_) => "<nothing within 5s>".into(),
        }
    }
}

type Side = (
    Conn<tokio::io::ReadHalf<DuplexStream>>,
    tokio::io::WriteHalf<DuplexStream>,
);

fn split(client: DuplexStream) -> Side {
    let (r, w) = tokio::io::split(client);
    (
        Conn {
            reader: BufReader::new(r),
        },
        w,
    )
}

async fn imap_signed_in<D>(dir: &Arc<D>, user: &str) -> Side
where
    D: mailrs_mail_protocols::backend::Directory,
{
    let (client, server) = tokio::io::duplex(1 << 16);
    tokio::spawn(mailrs_mail_protocols::imap::run(dir.clone(), server));
    let (mut r, mut w) = split(client);
    r.line().await;
    w.write_all(format!("a1 LOGIN {user} \"{PW}\"\r\n").as_bytes())
        .await
        .unwrap();
    assert_eq!(r.line().await, "a1 OK LOGIN completed");
    (r, w)
}

/// Publish the sign-out once every listener has subscribed.
fn sign_out(address: &str, listeners: usize) {
    let mut c = kevy_client::Connection::connect(KEVY).unwrap();
    for _ in 0..200 {
        // the publish that every listener hears is the one that counts
        if c.publish(signout::CHANNEL, b"probe@nowhere.example")
            .unwrap()
            >= listeners
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    signout::publish(&mut c, address).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ending_an_accounts_sign_ins_closes_its_open_connections() {
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
    for address in [ALICE, BOB] {
        client
            .add_account(&AddAccountRequest {
                address: address.into(),
                display_name: String::new(),
                password: PW.into(),
                password_hash: None,
            })
            .await
            .unwrap();
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

    // fastcore's protocols, wired as `protocols::spawn` wires them
    let dir = mailrs_fastcore::FastcoreDirectory::new(state, tmp.path().join("maildir"));
    let guard = mailrs_auth_guard::kevy::guard_for(Some(KEVY));
    let guarded = Arc::new(Guarded::new(Arc::new(dir), guard));
    let signins = guarded.signed_in();
    signout::listen(KEVY.to_string(), move |account| {
        signins.end(account);
    });
    let receiver = mailrs_receiver::start().await;

    let (mut idle, mut idle_w) = imap_signed_in(&guarded, ALICE).await;
    idle_w.write_all(b"a2 IDLE\r\n").await.unwrap();
    assert_eq!(idle.line().await, "+ idling");
    let (mut other, mut other_w) = imap_signed_in(&guarded, BOB).await;

    let (client, server) = tokio::io::duplex(1 << 16);
    tokio::spawn(mailrs_mail_protocols::pop3::session(
        guarded.clone(),
        server,
        None,
    ));
    let (mut pop, mut pop_w) = split(client);
    pop.line().await;
    pop_w
        .write_all(format!("USER {ALICE}\r\nPASS {PW}\r\n").as_bytes())
        .await
        .unwrap();
    assert_eq!(pop.line().await, "+OK send PASS");
    assert!(pop.line().await.starts_with("+OK"), "pop3 sign-in");

    let mut smtp = TcpStream::connect(("127.0.0.1", receiver.submission.port()))
        .await
        .unwrap();
    let (smtp_r, mut smtp_w) = smtp.split();
    let mut smtp_r = Conn {
        reader: BufReader::new(smtp_r),
    };
    smtp_r.line().await;
    smtp_w
        .write_all(b"EHLO sender.example.org\r\n")
        .await
        .unwrap();
    while !smtp_r.line().await.starts_with("250 ") {}
    let plain = base64::engine::general_purpose::STANDARD.encode(format!("\0{ALICE}\0{PW}"));
    smtp_w
        .write_all(format!("AUTH PLAIN {plain}\r\n").as_bytes())
        .await
        .unwrap();
    assert!(smtp_r.line().await.starts_with("235"), "smtp sign-in");

    tokio::task::spawn_blocking(|| sign_out(ALICE, 2))
        .await
        .unwrap();

    assert_eq!(
        idle.line().await,
        "* BYE [ALERT] Signed out: an administrator ended this account's sign-ins"
    );
    assert_eq!(idle.line().await, "<closed>", "imap stayed open after BYE");
    assert_eq!(
        pop.line().await,
        "-ERR Signed out: an administrator ended this account's sign-ins"
    );
    let r = smtp_r.line().await;
    assert!(r.starts_with("421"), "smtp: {r}");

    other_w.write_all(b"a3 NOOP\r\n").await.unwrap();
    assert_eq!(
        other.line().await,
        "a3 OK NOOP completed",
        "another account was signed out"
    );
}
