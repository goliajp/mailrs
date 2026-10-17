//! STLS on 110, implicit TLS on 995, and 110 without a certificate, over
//! real sockets and a real handshake.

use crate::tls::test_support::{Client, PASSWORD, USER, cert, listener, state};

#[tokio::test]
async fn stls_is_required_before_user_and_resets_the_capabilities() {
    let cert = cert();
    let (l, addr) = listener().await;
    tokio::spawn(super::serve_plain(l, state(), Some(cert.server.clone())));

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
    tokio::spawn(super::serve_tls(l, state(), cert.server.clone()));

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
    tokio::spawn(super::serve_plain(l, state(), None));

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
