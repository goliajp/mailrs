//! End-to-end SMTP AUTH tests (PLAIN / LOGIN, with and without TLS).

mod common;

use tokio::io::AsyncWriteExt;

use common::auth_mock::start_auth_server;
use common::smtp_mock::start_server;
use common::{connect, read_line, read_multiline, send};

#[tokio::test]
async fn e2e_auth_plain_inline_success() {
    use base64::Engine;

    let port = start_auth_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test").await;
    let ehlo = read_multiline(&mut reader).await;
    assert!(ehlo.contains("AUTH PLAIN LOGIN"));

    // AUTH PLAIN with inline credentials: base64(\0testuser\0testpass)
    let creds = base64::engine::general_purpose::STANDARD.encode(b"\x00testuser\x00testpass");
    send(&mut writer, &format!("AUTH PLAIN {creds}")).await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("235 "),
        "AUTH PLAIN with valid creds should return 235: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_auth_plain_inline_failure() {
    use base64::Engine;

    let port = start_auth_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    // AUTH PLAIN with wrong password
    let creds = base64::engine::general_purpose::STANDARD.encode(b"\x00testuser\x00wrongpass");
    send(&mut writer, &format!("AUTH PLAIN {creds}")).await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("535 "),
        "AUTH PLAIN with wrong creds should return 535: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_auth_plain_two_step() {
    use base64::Engine;

    let port = start_auth_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    // AUTH PLAIN without initial response -> 334 challenge
    send(&mut writer, "AUTH PLAIN").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("334 "),
        "AUTH PLAIN without creds should return 334 challenge: {resp}"
    );

    // send credentials in response
    let creds = base64::engine::general_purpose::STANDARD.encode(b"\x00testuser\x00testpass");
    send(&mut writer, &creds).await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("235 "),
        "credentials should be accepted: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_auth_login_success() {
    use base64::Engine;
    let b64 = &base64::engine::general_purpose::STANDARD;

    let port = start_auth_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    // AUTH LOGIN -> 334 username challenge
    send(&mut writer, "AUTH LOGIN").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("334 "),
        "AUTH LOGIN should return 334 username prompt: {resp}"
    );
    // challenge should be base64("Username:")
    assert!(
        resp.contains("VXNlcm5hbWU6"),
        "challenge should contain base64 of 'Username:': {resp}"
    );

    // send username
    send(&mut writer, &b64.encode(b"testuser")).await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("334 "),
        "should get 334 password prompt: {resp}"
    );
    // challenge should be base64("Password:")
    assert!(
        resp.contains("UGFzc3dvcmQ6"),
        "challenge should contain base64 of 'Password:': {resp}"
    );

    // send password
    send(&mut writer, &b64.encode(b"testpass")).await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("235 "),
        "AUTH LOGIN should succeed with 235: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_auth_login_failure() {
    use base64::Engine;
    let b64 = &base64::engine::general_purpose::STANDARD;

    let port = start_auth_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    send(&mut writer, "AUTH LOGIN").await;
    read_line(&mut reader).await; // 334 username

    send(&mut writer, &b64.encode(b"testuser")).await;
    read_line(&mut reader).await; // 334 password

    send(&mut writer, &b64.encode(b"badpassword")).await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("535 "),
        "AUTH LOGIN with wrong password should return 535: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_auth_requires_tls_by_default() {
    // default smtp server requires TLS for auth
    let port = start_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    // trying AUTH without TLS on default server should get 530
    send(&mut writer, "AUTH PLAIN dGVzdA==").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("530 "),
        "AUTH without TLS should return 530: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_authenticated_mail_flow() {
    use base64::Engine;

    let port = start_auth_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "EHLO test").await;
    read_multiline(&mut reader).await;

    // authenticate first
    let creds = base64::engine::general_purpose::STANDARD.encode(b"\x00testuser\x00testpass");
    send(&mut writer, &format!("AUTH PLAIN {creds}")).await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("235 "));

    // send mail after authentication
    send(&mut writer, "MAIL FROM:<testuser@test.local>").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("250 "),
        "MAIL FROM after auth should work: {resp}"
    );

    send(&mut writer, "RCPT TO:<recipient@test.local>").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("250 "));

    send(&mut writer, "DATA").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("354 "));

    writer
        .write_all(b"Subject: Auth test\r\n\r\nAuthenticated message\r\n.\r\n")
        .await
        .unwrap();
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("250 "),
        "DATA after auth should succeed: {resp}"
    );

    send(&mut writer, "QUIT").await;
    read_line(&mut reader).await;
}
