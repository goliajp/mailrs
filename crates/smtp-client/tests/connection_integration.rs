//! `SmtpConnection` integration tests against the in-process mock SMTP
//! server. Exercises the connect / EHLO / MAIL / RCPT / DATA / QUIT
//! lifecycle plus rejection mapping and connection-level failure paths.
//!
//! STARTTLS *success* coverage is intentionally deferred — verifying a
//! self-signed cert end-to-end requires an injection hook on
//! `SmtpConnection::try_starttls` (the production verifier is fixed to
//! `webpki-roots`). Tests below cover the STARTTLS-rejection /
//! handshake-fail paths that do NOT need cert validation to succeed.

mod common;

use std::time::Duration;

use common::mock_smtp::{Behavior, ensure_crypto_provider, spawn_mock_smtp};
use mailrs_smtp_client::{SmtpConnection, StarttlsResult, TimeoutConfig};

fn fast_timeouts() -> TimeoutConfig {
    TimeoutConfig {
        connect: Duration::from_millis(500),
        greeting: Duration::from_millis(500),
        command: Duration::from_millis(500),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn happy_path_deliver_then_quit() {
    let mock = spawn_mock_smtp(Behavior::Accept).await;
    let mut conn = SmtpConnection::connect("127.0.0.1", mock.addr.port())
        .await
        .expect("connect");
    assert!(!conn.is_tls(), "plaintext after connect");

    let ehlo = conn.ehlo("client.test").await.expect("ehlo");
    assert_eq!(ehlo.code, 250);

    let resp = conn
        .deliver(
            "sender@example.org",
            &["bob@example.com"],
            b"Subject: hi\r\n\r\nhello\r\n",
        )
        .await
        .expect("deliver");
    assert_eq!(resp.code, 250, "DATA success returns 2xx");

    conn.quit().await.expect("quit");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reject_5xx_after_mail_surfaces_response() {
    let mock = spawn_mock_smtp(Behavior::Reject5xxAfterMail).await;
    let mut conn = SmtpConnection::connect("127.0.0.1", mock.addr.port())
        .await
        .expect("connect");
    conn.ehlo("client.test").await.expect("ehlo");

    let resp = conn
        .deliver("sender@example.org", &["r@x.com"], b"body\r\n")
        .await
        .expect("deliver returns Ok with the rejection response");
    assert_eq!(resp.code, 550, "5xx-after-MAIL surfaces as 550 response");
    assert!(!resp.is_positive());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reject_5xx_after_rcpt_surfaces_response() {
    let mock = spawn_mock_smtp(Behavior::Reject5xxAfterRcpt).await;
    let mut conn = SmtpConnection::connect("127.0.0.1", mock.addr.port())
        .await
        .expect("connect");
    conn.ehlo("client.test").await.expect("ehlo");

    let resp = conn
        .deliver("sender@example.org", &["r@x.com"], b"body\r\n")
        .await
        .expect("deliver returns Ok with the rejection response");
    assert_eq!(resp.code, 550, "5xx after RCPT surfaces as 550");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn defer_4xx_after_rcpt_surfaces_response() {
    let mock = spawn_mock_smtp(Behavior::Defer4xxAfterRcpt).await;
    let mut conn = SmtpConnection::connect("127.0.0.1", mock.addr.port())
        .await
        .expect("connect");
    conn.ehlo("client.test").await.expect("ehlo");

    let resp = conn
        .deliver("sender@example.org", &["r@x.com"], b"body\r\n")
        .await
        .expect("deliver returns Ok with the deferral response");
    assert_eq!(resp.code, 450, "4xx after RCPT surfaces as 450");
    assert!(!resp.is_positive());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn close_mid_data_returns_unexpected_eof() {
    let mock = spawn_mock_smtp(Behavior::CloseMidData).await;
    let mut conn = SmtpConnection::connect("127.0.0.1", mock.addr.port())
        .await
        .expect("connect");
    conn.ehlo("client.test").await.expect("ehlo");

    // body is big enough that some bytes flush before the server kills
    // the socket; the read of the 250 response then sees EOF.
    let big_body = vec![b'x'; 8192];
    let err = conn
        .deliver("sender@example.org", &["r@x.com"], &big_body)
        .await
        .expect_err("connection drops mid-DATA");
    // either UnexpectedEof from the response read, or BrokenPipe /
    // ConnectionReset from the body write — all three are valid
    // surface forms of "remote went away mid-DATA".
    use std::io::ErrorKind;
    assert!(
        matches!(
            err.kind(),
            ErrorKind::UnexpectedEof
                | ErrorKind::BrokenPipe
                | ErrorKind::ConnectionReset
                | ErrorKind::ConnectionAborted
        ),
        "expected EOF / pipe-broken, got: {err:?}",
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn greeting_timeout_when_server_hangs() {
    let mock = spawn_mock_smtp(Behavior::HangAfterConnect).await;
    let result =
        SmtpConnection::connect_with_timeout("127.0.0.1", mock.addr.port(), &fast_timeouts()).await;
    match result {
        Ok(_) => panic!("hung greeting must NOT succeed"),
        Err(e) => assert_eq!(e.kind(), std::io::ErrorKind::TimedOut),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn starttls_handshake_fail_returns_structured_outcome() {
    ensure_crypto_provider();
    let mock = spawn_mock_smtp(Behavior::StarttlsHandshakeFail).await;
    let mut conn = SmtpConnection::connect("127.0.0.1", mock.addr.port())
        .await
        .expect("connect");
    conn.ehlo("client.test").await.expect("ehlo");

    let result = conn.try_starttls("mock.test").await;
    match result {
        StarttlsResult::HandshakeFailed { outcome, source } => {
            // server closed the socket immediately after `220 Ready`,
            // so rustls sees a transport-level error during the TLS
            // ClientHello round-trip. Any classification is fine —
            // the contract is just "structured failure, not panic".
            let _ = outcome.as_str();
            assert!(!source.to_string().is_empty());
        }
        StarttlsResult::Success(_) => panic!("expected HandshakeFailed, got Success"),
        StarttlsResult::Rejected { .. } => panic!("expected HandshakeFailed, got Rejected"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn starttls_rejected_by_server_keeps_connection_usable() {
    // AcceptNoStarttls advertises EHLO without STARTTLS; if the client
    // sends STARTTLS anyway, our mock replies 502 — which is the
    // "server rejected STARTTLS" surface. The connection should remain
    // plaintext-usable per the StarttlsResult::Rejected contract.
    ensure_crypto_provider();
    let mock = spawn_mock_smtp(Behavior::AcceptNoStarttls).await;
    let mut conn = SmtpConnection::connect("127.0.0.1", mock.addr.port())
        .await
        .expect("connect");
    conn.ehlo("client.test").await.expect("ehlo");

    let result = conn.try_starttls("mock.test").await;
    match result {
        StarttlsResult::Rejected {
            conn: mut returned,
            code,
            ..
        } => {
            assert!(code >= 400, "rejection code is 4xx/5xx, got {code}");
            // connection still works for plaintext follow-up
            let resp = returned
                .deliver("s@x.com", &["r@x.com"], b"body\r\n")
                .await
                .expect("deliver on returned conn");
            assert_eq!(resp.code, 250);
        }
        StarttlsResult::Success(_) => panic!("expected Rejected, got Success"),
        StarttlsResult::HandshakeFailed { .. } => panic!("expected Rejected, got HandshakeFailed"),
    }
}
