//! The socket half, against a server that answers from a script.
//!
//! A real IMAP server is not needed to pin the things that go wrong:
//! how a password with a quote in it is sent, whether a literal is read
//! by its announced length, and whether a refused login is told apart
//! from a server that is merely unhappy.

#![cfg(feature = "net")]

use std::sync::{Arc, Mutex};

use mailrs_imap_client::{NetError, Session, Tls};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

/// A server that replies to each tagged command in turn and records
/// what it was sent.
async fn server(script: Vec<&'static str>) -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&seen);
    tokio::spawn(async move {
        let (sock, _) = listener.accept().await.expect("accept");
        let (r, mut w) = sock.into_split();
        let mut r = BufReader::new(r);
        w.write_all(b"* OK ready\r\n").await.unwrap();
        for reply in script {
            let mut line = String::new();
            if r.read_line(&mut line).await.unwrap_or(0) == 0 {
                return;
            }
            recorded.lock().unwrap().push(line.clone());
            let tag = line.split(' ').next().unwrap_or("a001").to_string();
            let body = reply.replace("{tag}", &tag);
            w.write_all(body.as_bytes()).await.unwrap();
        }
    });
    (port, seen)
}

async fn connect(port: u16) -> Session {
    Session::connect("127.0.0.1", port, Tls::None)
        .await
        .expect("connect")
}

/// The one that reads as "wrong password" and is not.
///
/// Generated app passwords contain `"` and `\` often enough that an
/// unquoted LOGIN turns one into a syntax error — and the person is
/// told their password is wrong when it is right.
#[tokio::test]
async fn a_password_with_a_quote_is_escaped() {
    let (port, seen) = server(vec!["{tag} OK logged in\r\n"]).await;
    let mut s = connect(port).await;
    s.login("me@x.com", "pa\"ss\\word").await.expect("login");
    let sent = seen.lock().unwrap()[0].clone();
    assert!(
        sent.contains(r#""pa\"ss\\word""#),
        "the password was not escaped: {sent}"
    );
}

#[tokio::test]
async fn a_refused_login_is_permanent_rather_than_retried() {
    let (port, _) = server(vec![
        "{tag} NO [AUTHENTICATIONFAILED] Invalid credentials\r\n",
    ])
    .await;
    let mut s = connect(port).await;
    let err = s.login("me@x.com", "wrong").await.expect_err("accepted");
    assert!(matches!(err, NetError::Auth(_)), "{err:?}");
    assert!(err.is_permanent(), "a refused password would be retried");
}

/// A server that is merely unhappy must not be treated as a refused
/// password — that would stop syncing an account whose credentials are
/// fine, and only a person can undo it.
#[tokio::test]
async fn a_server_that_is_busy_is_not_a_refused_password() {
    let (port, _) = server(vec![
        "{tag} OK ok\r\n",
        "{tag} NO [INUSE] Mailbox is locked\r\n",
    ])
    .await;
    let mut s = connect(port).await;
    s.login("me@x.com", "right").await.expect("login");
    let err = s.select("INBOX").await.expect_err("accepted");
    assert!(!err.is_permanent(), "{err:?}");
}

/// A literal is read by the length the server announced. A body
/// contains every byte sequence a terminator could be made of, so
/// scanning for one truncates real mail.
#[tokio::test]
async fn a_body_holding_a_crlf_and_a_close_paren_survives() {
    let body = "Subject: x\r\n\r\nline one\r\n)\r\na001 OK not really\r\nend";
    let script = format!(
        "* 1 FETCH (UID 7 FLAGS (\\Seen) BODY[] {{{}}}\r\n{body})\r\n{{tag}} OK done\r\n",
        body.len()
    );
    let leaked: &'static str = Box::leak(script.into_boxed_str());
    let (port, _) = server(vec!["{tag} OK ok\r\n", leaked]).await;
    let mut s = connect(port).await;
    s.login("me@x.com", "p").await.expect("login");
    let got = s.fetch_full("1:*").await.expect("fetch");
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].0, 7);
    assert!(got[0].1.seen);
    assert_eq!(String::from_utf8_lossy(&got[0].2), body);
}

#[tokio::test]
async fn folders_are_read_with_what_they_are_for() {
    let (port, _) = server(vec![
        "{tag} OK ok\r\n",
        "* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n\
         * LIST (\\Noselect \\HasChildren) \"/\" \"[Gmail]\"\r\n\
         * LIST (\\All \\HasNoChildren) \"/\" \"[Gmail]/All Mail\"\r\n\
         {tag} OK done\r\n",
    ])
    .await;
    let mut s = connect(port).await;
    s.login("me@x.com", "p").await.expect("login");
    let folders = s.list().await.expect("list");
    assert_eq!(folders.len(), 3);
    assert!(
        folders
            .iter()
            .any(|f| f.name == "INBOX" && !f.selectable_is_false)
    );
    assert!(
        folders
            .iter()
            .any(|f| f.name == "[Gmail]" && f.selectable_is_false)
    );
    assert!(folders.iter().any(|f| f.is_all));
}

/// A server that hangs up is a connection problem, not a password one.
#[tokio::test]
async fn a_closed_connection_says_so() {
    let (port, _) = server(vec![]).await;
    let mut s = connect(port).await;
    let err = s.login("me@x.com", "p").await.expect_err("accepted");
    assert!(!err.is_permanent(), "{err:?}");
}
