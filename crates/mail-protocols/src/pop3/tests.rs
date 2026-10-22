//! STLS on 110, implicit TLS on 995, and 110 without a certificate, over
//! real sockets and a real handshake.

use crate::test_support::{Client, PASSWORD, USER, cert, directory, listener};

#[tokio::test]
async fn stls_is_required_before_user_and_resets_the_capabilities() {
    let cert = cert();
    let (l, addr) = listener().await;
    tokio::spawn(super::serve_plain(
        l,
        directory(),
        Some(cert.server.clone()),
    ));

    let mut c = Client::connect(addr).await;
    assert!(c.line().await.starts_with("+OK"));
    c.send("CAPA\r\n").await;
    let r = c.until(".").await;
    assert!(r.contains("\r\nSTLS\r\n"), "{r}");
    assert!(!r.contains("\r\nUSER\r\n"), "{r}");

    for cmd in [
        format!("USER {USER}"),
        format!("PASS {PASSWORD}"),
        "APOP x y".into(),
        "AUTH PLAIN".into(),
    ] {
        c.send(&format!("{cmd}\r\n")).await;
        let r = c.line().await;
        assert_eq!(r, "-ERR [AUTH] use STLS first\r\n", "{cmd}");
    }
    c.send("STAT\r\n").await;
    assert!(c.line().await.starts_with("-ERR"));

    // a command pipelined behind STLS was sent in clear and must not run
    c.send(&format!("STLS\r\nUSER {USER}\r\n")).await;
    assert!(c.line().await.starts_with("+OK"));
    let mut c = c.upgrade(&cert).await;

    c.send(&format!("PASS {PASSWORD}\r\n")).await;
    let r = c.line().await;
    assert_eq!(r, "-ERR USER first\r\n", "pipelined plaintext kept");
    c.send("CAPA\r\n").await;
    let r = c.until(".").await;
    assert!(r.contains("\r\nUSER\r\n"), "{r}");
    assert!(!r.contains("STLS"), "{r}");
    c.send("STLS\r\n").await;
    assert!(c.line().await.starts_with("-ERR"));
    c.send(&format!("USER {USER}\r\n")).await;
    assert!(c.line().await.starts_with("+OK"));
    c.send(&format!("PASS {PASSWORD}\r\n")).await;
    let r = c.line().await;
    assert!(r.starts_with("+OK "), "{r}");
    c.send("STAT\r\n").await;
    assert!(c.line().await.starts_with("+OK "));
    c.send("QUIT\r\n").await;
    assert!(c.line().await.starts_with("+OK"));
}

#[tokio::test]
async fn implicit_tls_offers_no_stls() {
    let cert = cert();
    let (l, addr) = listener().await;
    tokio::spawn(super::serve_tls(l, directory(), cert.server.clone()));

    let mut c = Client::connect_tls(addr, &cert).await;
    assert!(c.line().await.starts_with("+OK"));
    c.send("CAPA\r\n").await;
    let r = c.until(".").await;
    assert!(r.contains("\r\nUSER\r\n"), "{r}");
    assert!(!r.contains("STLS"), "{r}");
    c.send("STLS\r\n").await;
    assert!(c.line().await.starts_with("-ERR"));
    c.send(&format!("USER {USER}\r\n")).await;
    assert!(c.line().await.starts_with("+OK"));
    c.send(&format!("PASS {PASSWORD}\r\n")).await;
    assert!(c.line().await.starts_with("+OK "));
}

#[tokio::test]
async fn without_a_certificate_110_takes_user_in_clear() {
    let (l, addr) = listener().await;
    tokio::spawn(super::serve_plain(l, directory(), None));

    let mut c = Client::connect(addr).await;
    assert!(c.line().await.starts_with("+OK"));
    c.send("CAPA\r\n").await;
    let r = c.until(".").await;
    assert!(r.contains("\r\nUSER\r\n") && !r.contains("STLS"), "{r}");
    c.send(&format!("USER {USER}\r\n")).await;
    assert!(c.line().await.starts_with("+OK"));
    c.send(&format!("PASS {PASSWORD}\r\n")).await;
    assert!(c.line().await.starts_with("+OK "));
}

#[tokio::test]
async fn a_maildrop_is_numbered_from_one_and_dele_applies_at_quit() {
    let dir = directory();
    dir.deliver("Subject: one\r\n\r\nfirst\r\n");
    dir.deliver("Subject: two\r\n\r\n.dotted\r\n");
    let (l, addr) = listener().await;
    tokio::spawn(super::serve_plain(l, dir.clone(), None));

    let mut c = Client::connect(addr).await;
    c.line().await;
    c.send(&format!("USER {USER}\r\nPASS {PASSWORD}\r\n")).await;
    c.line().await;
    assert_eq!(c.line().await, "+OK 2 messages\r\n");
    c.send("LIST\r\n").await;
    assert_eq!(
        c.until(".").await,
        "+OK maildrop follows\r\n1 23\r\n2 25\r\n.\r\n"
    );
    c.send("UIDL\r\n").await;
    let r = c.until(".").await;
    assert_eq!(r, "+OK unique-id listing follows\r\n1 u1\r\n2 u2\r\n.\r\n");
    c.send("RETR 2\r\n").await;
    for want in [
        "+OK 25 octets\r\n",
        "Subject: two\r\n",
        "\r\n",
        "..dotted\r\n",
        ".\r\n",
    ] {
        assert_eq!(c.line().await, want, "byte-stuffed and terminated");
    }
    c.send("DELE 1\r\nSTAT\r\nQUIT\r\n").await;
    assert!(c.line().await.starts_with("+OK"));
    assert_eq!(c.line().await, "+OK 1 25\r\n");
    assert_eq!(c.line().await, "+OK bye\r\n");

    let mut c = Client::connect(addr).await;
    c.line().await;
    c.send(&format!("USER {USER}\r\nPASS {PASSWORD}\r\nTOP 1 0\r\n"))
        .await;
    c.line().await;
    assert_eq!(c.line().await, "+OK 1 messages\r\n");
    for want in [
        "+OK top of message follows\r\n",
        "Subject: two\r\n",
        "\r\n",
        ".\r\n",
    ] {
        assert_eq!(c.line().await, want);
    }
}
