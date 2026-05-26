//! End-to-end SMTP tests: greeting, EHLO/HELO, MAIL/RCPT/DATA, and global
//! commands (NOOP/VRFY/HELP/unknown).

mod common;

use tokio::io::AsyncWriteExt;

use common::auth_mock::start_auth_server;
use common::smtp_mock::start_server;
use common::{connect, read_line, read_multiline, send};

// ==== SMTP greeting tests ====

#[tokio::test]
async fn e2e_greeting_contains_esmtp() {
    let port = start_server().await;
    let (mut reader, _writer) = connect(port).await;

    let greeting = read_line(&mut reader).await;
    assert!(
        greeting.starts_with("220 "),
        "greeting must start with 220: {greeting}"
    );
    assert!(
        greeting.contains("ESMTP"),
        "greeting should contain ESMTP identifier: {greeting}"
    );
    assert!(
        greeting.contains("mx.test.local"),
        "greeting should contain server hostname: {greeting}"
    );
}

// ==== SMTP EHLO extension tests ====

#[tokio::test]
async fn e2e_ehlo_advertises_pipelining() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test.client").await;
    let ehlo = read_multiline(&mut reader).await;
    assert!(
        ehlo.contains("PIPELINING"),
        "EHLO must advertise PIPELINING: {ehlo}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_ehlo_advertises_size() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test.client").await;
    let ehlo = read_multiline(&mut reader).await;
    assert!(
        ehlo.contains("SIZE"),
        "EHLO must advertise SIZE extension: {ehlo}"
    );
    // default max_size is 52428800
    assert!(
        ehlo.contains("SIZE 52428800"),
        "EHLO SIZE should show default max size: {ehlo}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_ehlo_multiline_format() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test.client").await;
    let ehlo = read_multiline(&mut reader).await;

    // first line should be "250-hostname"
    let lines: Vec<&str> = ehlo.lines().collect();
    assert!(
        lines.len() >= 2,
        "EHLO response should have multiple lines: {ehlo}"
    );
    assert!(
        lines[0].starts_with("250-"),
        "first EHLO line must use continuation: {ehlo}"
    );
    assert!(
        lines[0].contains("mx.test.local"),
        "first EHLO line must contain hostname: {ehlo}"
    );
    // last line should use "250 " (space, not dash)
    let last = lines.last().unwrap();
    assert!(
        last.starts_with("250 "),
        "last EHLO line must use space separator: {ehlo}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_ehlo_no_auth_without_tls() {
    // default config requires TLS for auth, so AUTH should not be advertised
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test.client").await;
    let ehlo = read_multiline(&mut reader).await;
    assert!(
        !ehlo.contains("AUTH"),
        "AUTH should not be advertised without TLS: {ehlo}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_ehlo_auth_when_tls_not_required() {
    // auth server has require_tls_for_auth = false
    let port = start_auth_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test.client").await;
    let ehlo = read_multiline(&mut reader).await;
    assert!(
        ehlo.contains("AUTH PLAIN LOGIN"),
        "AUTH PLAIN LOGIN should be advertised when TLS not required: {ehlo}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

// ==== SMTP HELO fallback ====

#[tokio::test]
async fn e2e_helo_basic() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "HELO old.client").await;
    let resp = read_multiline(&mut reader).await;
    assert!(resp.contains("250"), "HELO should return 250: {resp}");

    // should still be able to send mail after HELO
    send(&mut writer, "MAIL FROM:<sender@example.com>").await;
    let mail_resp = read_line(&mut reader).await;
    assert!(
        mail_resp.starts_with("250 "),
        "MAIL FROM after HELO should work: {mail_resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

// ==== SMTP MAIL FROM / RCPT TO / DATA flow tests ====

#[tokio::test]
async fn e2e_rcpt_to_without_mail_from() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    // RCPT TO without MAIL FROM should fail
    send(&mut writer, "RCPT TO:<bob@test.local>").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("503 "),
        "RCPT TO without MAIL FROM should return 503: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_data_without_rcpt_to() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    send(&mut writer, "MAIL FROM:<a@b>").await;
    read_line(&mut reader).await;

    // DATA without RCPT TO should fail
    send(&mut writer, "DATA").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("503 "),
        "DATA without RCPT TO should return 503: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_multiple_rcpt_to() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    send(&mut writer, "MAIL FROM:<sender@example.com>").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("250 "));

    // multiple recipients
    send(&mut writer, "RCPT TO:<alice@test.local>").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("250 "),
        "first RCPT TO should succeed: {resp}"
    );

    send(&mut writer, "RCPT TO:<bob@test.local>").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("250 "),
        "second RCPT TO should succeed: {resp}"
    );

    send(&mut writer, "RCPT TO:<carol@test.local>").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("250 "),
        "third RCPT TO should succeed: {resp}"
    );

    send(&mut writer, "DATA").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("354 "));

    writer
        .write_all(b"Subject: Multi-rcpt\r\n\r\nHello all\r\n.\r\n")
        .await
        .unwrap();
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("250 "),
        "DATA should succeed with multiple rcpts: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_null_reverse_path() {
    // bounce messages use MAIL FROM:<> (null sender)
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    send(&mut writer, "MAIL FROM:<>").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("250 "),
        "null reverse path should be accepted: {resp}"
    );

    send(&mut writer, "RCPT TO:<postmaster>").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("250 "),
        "RCPT TO postmaster should work: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_two_transactions_one_connection() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    // first transaction
    send(&mut writer, "MAIL FROM:<first@example.com>").await;
    read_line(&mut reader).await;
    send(&mut writer, "RCPT TO:<bob@test.local>").await;
    read_line(&mut reader).await;
    send(&mut writer, "DATA").await;
    read_line(&mut reader).await;
    writer
        .write_all(b"Subject: First\r\n\r\nFirst message\r\n.\r\n")
        .await
        .unwrap();
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("250 "),
        "first transaction should succeed: {resp}"
    );

    // second transaction without re-EHLO
    send(&mut writer, "MAIL FROM:<second@example.com>").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("250 "),
        "second MAIL FROM should succeed after completed transaction: {resp}"
    );

    send(&mut writer, "RCPT TO:<alice@test.local>").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("250 "));

    send(&mut writer, "DATA").await;
    read_line(&mut reader).await;
    writer
        .write_all(b"Subject: Second\r\n\r\nSecond message\r\n.\r\n")
        .await
        .unwrap();
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("250 "),
        "second transaction should succeed: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

// ==== SMTP global commands ====

#[tokio::test]
async fn e2e_noop_any_state() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    // NOOP before EHLO
    send(&mut writer, "NOOP").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("250 "), "NOOP in Connected state: {resp}");

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    // NOOP after EHLO
    send(&mut writer, "NOOP").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("250 "), "NOOP in Greeted state: {resp}");

    send(&mut writer, "MAIL FROM:<a@b>").await;
    read_line(&mut reader).await;

    // NOOP in MailFrom state
    send(&mut writer, "NOOP").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("250 "), "NOOP in MailFrom state: {resp}");

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_vrfy_returns_252() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "VRFY user@example.com").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("252 "), "VRFY should return 252: {resp}");

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_help_returns_214() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "HELP").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("214 "), "HELP should return 214: {resp}");

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_unknown_command_returns_500() {
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "XYZZY").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("500 "),
        "unknown command should return 500: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}
