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

/// The cheap half of a re-alignment: identities, no bodies.
#[tokio::test]
async fn envelopes_come_back_without_bodies() {
    let script = concat!(
        r#"* 1 FETCH (UID 7 ENVELOPE ("d" "s" (("A" NIL "a" "x.com")) (("A" NIL "a" "x.com")) NIL (("B" NIL "b" "y.com")) NIL NIL NIL "<one@x.com>"))"#,
        "\r\n",
        r#"* 2 FETCH (UID 9 ENVELOPE ("d" "s" (("A" NIL "a" "x.com")) (("A" NIL "a" "x.com")) NIL (("B" NIL "b" "y.com")) NIL NIL NIL "<two@x.com>"))"#,
        "\r\n{tag} OK done\r\n",
    );
    let (port, _) = server(vec!["{tag} OK ok\r\n", script]).await;
    let mut s = connect(port).await;
    s.login("me@x.com", "p").await.expect("login");
    let got = s.fetch_envelopes("1:*").await.expect("envelopes");
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].uid, 7);
    assert_eq!(got[1].message_id, "<two@x.com>");
}

/// A line this cannot read is skipped, not guessed at — and the
/// messages around it still arrive. A wrong Message-ID would file a
/// message as a different one or merge it into somebody else's thread.
#[tokio::test]
async fn an_unreadable_envelope_does_not_lose_the_others() {
    let script = concat!(
        r#"* 1 FETCH (UID 7 ENVELOPE ("d" "s" (("A" NIL "a" "x.com")) (("A" NIL "a" "x.com")) NIL (("B" NIL "b" "y.com")) NIL NIL NIL "<one@x.com>"))"#,
        "\r\n",
        "* 2 FETCH (UID 8 ENVELOPE (truncated\r\n",
        r#"* 3 FETCH (UID 9 ENVELOPE ("d" "s" (("A" NIL "a" "x.com")) (("A" NIL "a" "x.com")) NIL (("B" NIL "b" "y.com")) NIL NIL NIL "<three@x.com>"))"#,
        "\r\n{tag} OK done\r\n",
    );
    let (port, _) = server(vec!["{tag} OK ok\r\n", script]).await;
    let mut s = connect(port).await;
    s.login("me@x.com", "p").await.expect("login");
    let got = s.fetch_envelopes("1:*").await.expect("envelopes");
    assert_eq!(got.len(), 2, "the readable ones were lost with the bad one");
    assert_eq!(got[0].message_id, "<one@x.com>");
    assert_eq!(got[1].message_id, "<three@x.com>");
}

/// Marking read on the server, and the two things about the command
/// that are choices rather than syntax.
///
/// `.SILENT`, because the plain form answers with an untagged FETCH
/// for every uid — replies that would have to be read and discarded,
/// and anything left in the buffer desynchronises the next command.
/// And one command for the whole set: a STORE per message turns
/// reading a thread into a conversation of its own.
#[tokio::test]
async fn marking_read_is_one_silent_command_for_the_whole_set() {
    let (port, seen) = server(vec!["{tag} OK stored\r\n"]).await;
    let mut s = connect(port).await;
    s.store_seen(&[4390, 4391, 4400]).await.expect("store");

    let sent = seen.lock().unwrap()[0].clone();
    assert!(
        sent.contains("UID STORE 4390,4391,4400"),
        "not one command for the set: {sent}"
    );
    assert!(
        sent.contains("+FLAGS.SILENT"),
        "the non-silent form leaves replies in the buffer: {sent}"
    );
    assert!(
        sent.contains("\\Seen"),
        "the flag did not survive escaping: {sent}"
    );
    assert_eq!(
        seen.lock().unwrap().len(),
        1,
        "more than one command went out"
    );
}

/// Nothing to say means nothing is sent — a pass with no notes must
/// not select a folder and issue an empty STORE.
#[tokio::test]
async fn nothing_to_mark_sends_nothing() {
    let (port, seen) = server(vec!["{tag} OK unused\r\n"]).await;
    let mut s = connect(port).await;
    s.store_seen(&[]).await.expect("store");
    assert!(seen.lock().unwrap().is_empty(), "an empty STORE went out");
}

/// A refusal is reported rather than swallowed: the note stays queued
/// and is carried on the next pass.
#[tokio::test]
async fn a_refused_store_is_an_error() {
    let (port, _) = server(vec!["{tag} NO [CANNOT] read-only mailbox\r\n"]).await;
    let mut s = connect(port).await;
    assert!(
        s.store_seen(&[1]).await.is_err(),
        "a refusal read as success"
    );
}
