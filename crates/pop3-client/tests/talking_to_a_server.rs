//! The socket half, against a server that answers from a script.

#![cfg(feature = "net")]

use std::sync::{Arc, Mutex};

use mailrs_pop3_client::{NetError, Session, Tls};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

/// A server that replies to each command in turn and records what it
/// was sent.
async fn server(script: Vec<&'static str>) -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&seen);
    tokio::spawn(async move {
        let (sock, _) = listener.accept().await.expect("accept");
        let (r, mut w) = sock.into_split();
        let mut r = BufReader::new(r);
        w.write_all(b"+OK POP3 ready\r\n").await.unwrap();
        for reply in script {
            let mut line = String::new();
            if r.read_line(&mut line).await.unwrap_or(0) == 0 {
                return;
            }
            recorded.lock().unwrap().push(line);
            w.write_all(reply.as_bytes()).await.unwrap();
        }
    });
    (port, seen)
}

async fn connect(port: u16) -> Session {
    Session::connect("127.0.0.1", port, Tls::None)
        .await
        .expect("connect")
}

#[tokio::test]
async fn a_refused_password_is_permanent_rather_than_retried() {
    let (port, _) = server(vec!["+OK\r\n", "-ERR [AUTH] Authentication failed\r\n"]).await;
    let mut s = connect(port).await;
    let err = s.login("me@x.com", "wrong").await.expect_err("accepted");
    assert!(matches!(err, NetError::Auth(_)), "{err:?}");
    assert!(err.is_permanent());
}

/// A server that is merely unhappy must not be treated as a refused
/// password — that stops syncing an account whose credentials are
/// fine, and only a person can undo it.
///
/// POP3 gives no response codes, so this is the boundary: **`PASS` is
/// where a refusal means the credential**, and anything after it does
/// not.
#[tokio::test]
async fn a_locked_mailbox_after_login_is_not_a_refused_password() {
    let (port, _) = server(vec![
        "+OK\r\n",
        "+OK logged in\r\n",
        "-ERR mailbox locked, try later\r\n",
    ])
    .await;
    let mut s = connect(port).await;
    s.login("me@x.com", "right").await.expect("login");
    let err = s.uidl().await.expect_err("accepted");
    assert!(!err.is_permanent(), "{err:?}");
}

/// A server with no `UIDL` is a named, permanent failure: its mail
/// cannot be told apart between syncs, and the person needs to know at
/// set-up rather than have the mailbox re-downloaded forever.
#[tokio::test]
async fn a_server_without_uidl_says_so_by_name() {
    let (port, _) = server(vec!["+OK\r\n", "+OK\r\n", "-ERR unknown command\r\n"]).await;
    let mut s = connect(port).await;
    s.login("me@x.com", "p").await.expect("login");
    let err = s.uidl().await.expect_err("accepted");
    assert!(matches!(err, NetError::NoUidl), "{err:?}");
    assert!(err.is_permanent());
}

#[tokio::test]
async fn a_uidl_listing_is_read() {
    let (port, _) = server(vec![
        "+OK\r\n",
        "+OK\r\n",
        "+OK 2 messages\r\n1 abc\r\n2 def\r\n.\r\n",
    ])
    .await;
    let mut s = connect(port).await;
    s.login("me@x.com", "p").await.expect("login");
    let uids = s.uidl().await.expect("uidl");
    assert_eq!(uids.len(), 2);
    assert_eq!(uids[1].uid, "def");
}

/// The one that corrupts mail. RFC 1939 §3: a body line beginning with
/// `.` is sent doubled, and a client that does not undo it damages
/// every message containing one — quoted mail and `..` in code both
/// produce them.
#[tokio::test]
async fn a_body_line_beginning_with_a_dot_arrives_undoubled() {
    let (port, _) = server(vec![
        "+OK\r\n",
        "+OK\r\n",
        "+OK 40 octets\r\nSubject: x\r\n\r\n..hidden\r\n...three\r\nend\r\n.\r\n",
    ])
    .await;
    let mut s = connect(port).await;
    s.login("me@x.com", "p").await.expect("login");
    let body = String::from_utf8(s.retr(1).await.expect("retr")).expect("utf8");
    assert!(body.contains("\r\n.hidden\r\n"), "{body:?}");
    assert!(body.contains("\r\n..three\r\n"), "{body:?}");
    assert!(body.ends_with("end"), "{body:?}");
}

/// A server that hangs up is a connection problem, not a password one.
#[tokio::test]
async fn a_closed_connection_says_so() {
    let (port, _) = server(vec![]).await;
    let mut s = connect(port).await;
    let err = s.login("me@x.com", "p").await.expect_err("accepted");
    assert!(!err.is_permanent(), "{err:?}");
}
