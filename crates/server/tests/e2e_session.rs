//! End-to-end SMTP session-management tests: QUIT/RSET/EHLO state machine,
//! multi-session, pipelining, rapid connect/disconnect.

mod common;

use tokio::io::AsyncWriteExt;

use common::smtp_mock::start_server;
use common::{connect, read_line, read_multiline, send};

#[tokio::test]
async fn e2e_quit_before_ehlo() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    // QUIT without EHLO should still work
    send(&mut writer, "QUIT").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("221 "),
        "QUIT before EHLO should return 221: {resp}"
    );
}

#[tokio::test]
async fn e2e_ehlo_resets_session() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO first.test").await;
    read_multiline(&mut reader).await;

    send(&mut writer, "MAIL FROM:<a@b>").await;
    read_line(&mut reader).await;

    send(&mut writer, "RCPT TO:<x@test.local>").await;
    read_line(&mut reader).await;

    // second EHLO should reset the session
    send(&mut writer, "EHLO second.test").await;
    read_multiline(&mut reader).await;

    // RCPT TO should fail because transaction was reset
    send(&mut writer, "RCPT TO:<y@test.local>").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("503 "),
        "RCPT TO after EHLO reset should return 503: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_rset_in_connected_state() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    // RSET before EHLO should return 250
    send(&mut writer, "RSET").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("250 "),
        "RSET in Connected state should return 250: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_rapid_connect_disconnect() {
    let port = start_server().await;

    // rapidly connect and disconnect multiple times
    for _ in 0..5 {
        let (mut reader, mut writer) = connect(port).await;
        let greeting = read_line(&mut reader).await;
        assert!(greeting.starts_with("220 "));

        send(&mut writer, "QUIT").await;
        let resp = read_line(&mut reader).await;
        assert!(resp.starts_with("221 "));
    }
}

// ==== original tests ====

#[tokio::test]
async fn e2e_basic_session() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;

    // greeting
    let greeting = read_line(&mut reader).await;
    assert!(
        greeting.starts_with("220 "),
        "expected 220 greeting: {greeting}"
    );

    // EHLO
    send(&mut writer, "EHLO test.client").await;
    let ehlo_resp = read_multiline(&mut reader).await;
    assert!(
        ehlo_resp.contains("250"),
        "expected 250 EHLO response: {ehlo_resp}"
    );

    // MAIL FROM
    send(&mut writer, "MAIL FROM:<alice@example.com>").await;
    let mail_resp = read_line(&mut reader).await;
    assert!(mail_resp.starts_with("250 "), "expected 250: {mail_resp}");

    // RCPT TO
    send(&mut writer, "RCPT TO:<bob@test.local>").await;
    let rcpt_resp = read_line(&mut reader).await;
    assert!(rcpt_resp.starts_with("250 "), "expected 250: {rcpt_resp}");

    // DATA
    send(&mut writer, "DATA").await;
    let data_resp = read_line(&mut reader).await;
    assert!(data_resp.starts_with("354 "), "expected 354: {data_resp}");

    // message body
    writer
        .write_all(b"Subject: Test\r\n\r\nHello world\r\n.\r\n")
        .await
        .unwrap();
    let queued = read_line(&mut reader).await;
    assert!(queued.starts_with("250 "), "expected 250 queued: {queued}");

    // QUIT
    send(&mut writer, "QUIT").await;
    let quit_resp = read_line(&mut reader).await;
    assert!(quit_resp.starts_with("221 "), "expected 221: {quit_resp}");
}

#[tokio::test]
async fn e2e_bad_sequence() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;

    read_line(&mut reader).await; // greeting

    // MAIL FROM without EHLO
    send(&mut writer, "MAIL FROM:<alice@example.com>").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("503 "), "expected 503: {resp}");

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_multiple_sessions() {
    let port = start_server().await;

    let handles: Vec<_> = (0..3)
        .map(|i| {
            tokio::spawn(async move {
                let (mut reader, mut writer) = connect(port).await;
                read_line(&mut reader).await; // greeting

                send(&mut writer, &format!("EHLO client{i}.test")).await;
                read_multiline(&mut reader).await;

                send(&mut writer, &format!("MAIL FROM:<user{i}@example.com>")).await;
                let resp = read_line(&mut reader).await;
                assert!(resp.starts_with("250 "), "session {i} MAIL failed: {resp}");

                send(&mut writer, &format!("RCPT TO:<rcpt{i}@test.local>")).await;
                let resp = read_line(&mut reader).await;
                assert!(resp.starts_with("250 "), "session {i} RCPT failed: {resp}");

                send(&mut writer, "DATA").await;
                read_line(&mut reader).await;

                writer
                    .write_all(format!("Subject: Test {i}\r\n\r\nBody {i}\r\n.\r\n").as_bytes())
                    .await
                    .unwrap();
                let resp = read_line(&mut reader).await;
                assert!(resp.starts_with("250 "), "session {i} DATA failed: {resp}");

                send(&mut writer, "QUIT").await;
                read_line(&mut reader).await;
            })
        })
        .collect();

    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn e2e_pipelining() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;

    read_line(&mut reader).await; // greeting

    send(&mut writer, "EHLO pipeline.test").await;
    let ehlo = read_multiline(&mut reader).await;
    assert!(
        ehlo.contains("PIPELINING"),
        "server should advertise PIPELINING"
    );

    send(&mut writer, "MAIL FROM:<a@b>").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("250 "));

    send(&mut writer, "RCPT TO:<x@test.local>").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("250 "));

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_rset_mid_transaction() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;

    read_line(&mut reader).await; // greeting

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    send(&mut writer, "MAIL FROM:<a@b>").await;
    read_line(&mut reader).await;

    send(&mut writer, "RCPT TO:<x@test.local>").await;
    read_line(&mut reader).await;

    // RSET should reset to Greeted
    send(&mut writer, "RSET").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("250 "), "expected 250 for RSET: {resp}");

    // should be able to start new transaction
    send(&mut writer, "MAIL FROM:<new@sender>").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("250 "),
        "MAIL after RSET should work: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}
