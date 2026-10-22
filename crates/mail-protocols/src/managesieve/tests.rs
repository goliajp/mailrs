//! STARTTLS on 4190 and 4190 without a certificate, over real sockets
//! and a real handshake.

use std::time::Duration;

use base64::Engine;

use crate::test_support::{Client, PASSWORD, USER, cert, directory, listener};

fn authenticate(pass: &str) -> String {
    let ir = base64::engine::general_purpose::STANDARD.encode(format!("\0{USER}\0{pass}"));
    format!("AUTHENTICATE \"PLAIN\" \"{ir}\"\r\n")
}

#[tokio::test]
async fn starttls_is_required_before_authenticate_and_resends_the_capabilities() {
    let cert = cert();
    let (l, addr) = listener().await;
    tokio::spawn(super::serve_plain(
        l,
        directory(),
        Some(cert.server.clone()),
    ));

    let mut c = Client::connect(addr).await;
    let caps = c.until("OK").await;
    assert!(caps.contains("\"STARTTLS\"\r\n"), "{caps}");
    assert!(caps.contains("\"SASL\" \"\"\r\n"), "{caps}");
    c.send("CAPABILITY\r\n").await;
    assert_eq!(c.until("OK").await, caps);

    c.send(&authenticate(PASSWORD)).await;
    let r = c.line().await;
    assert_eq!(r, "NO (ENCRYPT-NEEDED) \"use STARTTLS first\"\r\n");
    c.send("LISTSCRIPTS\r\n").await;
    assert!(c.line().await.starts_with("NO "));

    // a command pipelined behind STARTTLS was sent in clear and must not run
    c.send("STARTTLS\r\nNOOP\r\n").await;
    assert!(c.line().await.starts_with("OK "));
    let mut c = c.upgrade(&cert).await;

    // RFC 5804 §2.2: the capabilities again, unasked, before anything else
    let caps = tokio::time::timeout(Duration::from_secs(5), c.until("OK"))
        .await
        .expect("no capabilities after STARTTLS");
    assert!(caps.contains("\"SASL\" \"PLAIN\"\r\n"), "{caps}");
    assert!(!caps.contains("STARTTLS"), "{caps}");
    c.send("STARTTLS\r\n").await;
    let r = c.line().await;
    assert!(r.starts_with("NO "), "pipelined plaintext answered: {r}");
    c.send(&authenticate("wrong")).await;
    assert!(c.line().await.starts_with("NO "));
    c.send(&authenticate(PASSWORD)).await;
    let r = c.line().await;
    assert!(r.starts_with("OK "), "{r}");
}

#[tokio::test]
async fn without_a_certificate_4190_authenticates_in_clear() {
    let (l, addr) = listener().await;
    tokio::spawn(super::serve_plain(l, directory(), None));

    let mut c = Client::connect(addr).await;
    let caps = c.until("OK").await;
    assert!(caps.contains("\"SASL\" \"PLAIN\"\r\n"), "{caps}");
    assert!(!caps.contains("STARTTLS"), "{caps}");
    c.send("STARTTLS\r\n").await;
    assert!(c.line().await.starts_with("NO "));
    c.send(&authenticate(PASSWORD)).await;
    let r = c.line().await;
    assert!(r.starts_with("OK "), "{r}");
}

#[test]
fn unquote_strips_quotes() {
    assert_eq!(super::unquote("\"PLAIN\""), "PLAIN");
    assert_eq!(super::unquote("  bare  "), "bare");
}

#[test]
fn greeting_advertises_plain_and_version() {
    let g = super::greeting(false);
    assert!(g.contains("\"SASL\" \"PLAIN\""));
    assert!(g.contains("\"VERSION\" \"1.0\""));
    assert!(g.ends_with("OK\r\n"));
}
