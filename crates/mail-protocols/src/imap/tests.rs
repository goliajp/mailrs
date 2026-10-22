//! STARTTLS on 143, implicit TLS on 993, and 143 without a certificate,
//! over real sockets and a real handshake.

use base64::Engine;

use crate::test_support::{Client, PASSWORD, USER, cert, directory, listener};

fn plain(user: &str, pass: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(format!("\0{user}\0{pass}"))
}

fn tagged(reply: &str) -> &str {
    reply.lines().last().unwrap_or("")
}

#[tokio::test]
async fn starttls_is_required_before_login_and_resets_the_capabilities() {
    let cert = cert();
    let (l, addr) = listener().await;
    tokio::spawn(super::serve_plain(
        l,
        directory(),
        Some(cert.server.clone()),
    ));

    let mut c = Client::connect(addr).await;
    let greeting = c.line().await;
    assert!(greeting.contains(" STARTTLS "), "{greeting}");
    assert!(greeting.contains(" LOGINDISABLED "), "{greeting}");
    assert!(!greeting.contains("AUTH="), "{greeting}");

    c.send("a1 CAPABILITY\r\n").await;
    let r = c.until("a1 ").await;
    assert!(r.contains(" STARTTLS LOGINDISABLED "), "{r}");
    assert!(!r.contains("AUTH="), "{r}");

    c.send(&format!("a2 LOGIN {USER} \"{PASSWORD}\"\r\n")).await;
    let r = c.until("a2 ").await;
    assert!(tagged(&r).starts_with("a2 NO [PRIVACYREQUIRED]"), "{r}");
    c.send(&format!(
        "a3 AUTHENTICATE PLAIN {}\r\n",
        plain(USER, PASSWORD)
    ))
    .await;
    let r = c.until("a3 ").await;
    assert!(tagged(&r).starts_with("a3 NO [PRIVACYREQUIRED]"), "{r}");
    c.send("a4 SELECT INBOX\r\n").await;
    let r = c.until("a4 ").await;
    assert!(tagged(&r).starts_with("a4 NO "), "{r}");

    // a command pipelined behind STARTTLS was sent in clear and must not run
    c.send("a5 STARTTLS\r\na6 NOOP\r\n").await;
    let r = c.until("a5 ").await;
    assert!(tagged(&r).starts_with("a5 OK "), "{r}");
    let mut c = c.upgrade(&cert).await;

    c.send("b1 NOOP\r\n").await;
    let r = c.line().await;
    assert!(r.starts_with("b1 OK "), "pipelined plaintext answered: {r}");
    c.send("b2 CAPABILITY\r\n").await;
    let r = c.until("b2 ").await;
    assert!(r.contains(" AUTH=PLAIN "), "{r}");
    assert!(!r.contains("STARTTLS"), "{r}");
    assert!(!r.contains("LOGINDISABLED"), "{r}");
    c.send("b3 STARTTLS\r\n").await;
    let r = c.until("b3 ").await;
    assert!(tagged(&r).starts_with("b3 BAD "), "{r}");
    c.send(&format!("b4 LOGIN {USER} wrong\r\n")).await;
    let r = c.until("b4 ").await;
    assert!(tagged(&r).starts_with("b4 NO "), "{r}");
    c.send(&format!("b5 LOGIN {USER} \"{PASSWORD}\"\r\n")).await;
    let r = c.until("b5 ").await;
    assert!(tagged(&r).starts_with("b5 OK "), "{r}");
}

#[tokio::test]
async fn implicit_tls_offers_no_starttls_and_takes_authenticate_plain() {
    let cert = cert();
    let (l, addr) = listener().await;
    tokio::spawn(super::serve_tls(l, directory(), cert.server.clone()));

    let mut c = Client::connect_tls(addr, &cert).await;
    let greeting = c.line().await;
    assert!(greeting.contains(" AUTH=PLAIN "), "{greeting}");
    assert!(!greeting.contains("STARTTLS"), "{greeting}");
    assert!(!greeting.contains("LOGINDISABLED"), "{greeting}");

    c.send("a1 STARTTLS\r\n").await;
    let r = c.until("a1 ").await;
    assert!(tagged(&r).starts_with("a1 BAD "), "{r}");

    c.send(&format!(
        "a2 AUTHENTICATE PLAIN {}\r\n",
        plain(USER, "wrong")
    ))
    .await;
    let r = c.until("a2 ").await;
    assert!(tagged(&r).starts_with("a2 NO "), "{r}");

    // without an initial response the server asks for it
    c.send("a3 AUTHENTICATE PLAIN\r\n").await;
    let r = c.line().await;
    assert!(r.starts_with("+"), "{r}");
    c.send(&format!("{}\r\n", plain(USER, PASSWORD))).await;
    let r = c.until("a3 ").await;
    assert!(tagged(&r).starts_with("a3 OK "), "{r}");
    c.send("a4 SELECT INBOX\r\n").await;
    let r = c.until("a4 ").await;
    assert!(tagged(&r).starts_with("a4 OK "), "{r}");
}

#[tokio::test]
async fn without_a_certificate_143_takes_logins_in_clear() {
    let (l, addr) = listener().await;
    tokio::spawn(super::serve_plain(l, directory(), None));

    let mut c = Client::connect(addr).await;
    let greeting = c.line().await;
    assert!(greeting.contains(" AUTH=PLAIN "), "{greeting}");
    assert!(!greeting.contains("STARTTLS"), "{greeting}");
    c.send("a1 STARTTLS\r\n").await;
    let r = c.until("a1 ").await;
    assert!(tagged(&r).starts_with("a1 BAD "), "{r}");
    c.send(&format!("a2 LOGIN {USER} \"{PASSWORD}\"\r\n")).await;
    let r = c.until("a2 ").await;
    assert!(tagged(&r).starts_with("a2 OK "), "{r}");

    // every advertised verb answers: NAMESPACE was listed long before it was
    assert!(greeting.contains(" NAMESPACE "), "{greeting}");
    c.send("a3 NAMESPACE\r\n").await;
    let r = c.until("a3 ").await;
    assert!(r.contains("* NAMESPACE ((\"\" \"/\")) NIL NIL"), "{r}");
    assert!(tagged(&r).starts_with("a3 OK "), "{r}");
}
