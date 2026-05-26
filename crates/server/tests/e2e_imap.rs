//! End-to-end IMAP tests: greeting, CAPABILITY, LOGIN, LOGOUT, NOOP, LIST.

mod common;

use common::imap_mock::start_imap_server;
use common::{connect, read_line, send};

#[tokio::test]
async fn e2e_imap_greeting_format() {
    let port = start_imap_server().await;
    let (mut reader, _writer) = connect(port).await;

    let greeting = read_line(&mut reader).await;
    assert!(
        greeting.starts_with("* OK"),
        "IMAP greeting must start with '* OK': {greeting}"
    );
    assert!(
        greeting.contains("IMAP4rev1"),
        "IMAP greeting must contain IMAP4rev1: {greeting}"
    );
    assert!(
        greeting.contains("imap.test.local"),
        "IMAP greeting must contain hostname: {greeting}"
    );
    assert!(
        greeting.ends_with("\r\n"),
        "IMAP greeting must end with CRLF"
    );
}

#[tokio::test]
async fn e2e_imap_capability() {
    let port = start_imap_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await; // greeting

    send(&mut writer, "a001 CAPABILITY").await;
    // read capability response (untagged) + OK
    let cap_line = read_line(&mut reader).await;
    assert!(
        cap_line.starts_with("* CAPABILITY"),
        "CAPABILITY response should be untagged: {cap_line}"
    );
    assert!(
        cap_line.contains("IMAP4rev1"),
        "CAPABILITY must include IMAP4rev1: {cap_line}"
    );
    assert!(
        cap_line.contains("AUTH=PLAIN"),
        "CAPABILITY must include AUTH=PLAIN: {cap_line}"
    );
    assert!(
        cap_line.contains("IDLE"),
        "CAPABILITY must include IDLE: {cap_line}"
    );

    let ok_line = read_line(&mut reader).await;
    assert!(
        ok_line.starts_with("a001 OK"),
        "tagged OK response expected: {ok_line}"
    );

    send(&mut writer, "a002 LOGOUT").await;
    read_line(&mut reader).await; // BYE
    read_line(&mut reader).await; // OK
}

#[tokio::test]
async fn e2e_imap_login_success() {
    let port = start_imap_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "a001 LOGIN testuser testpass").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("a001 OK"),
        "LOGIN with valid creds should return OK: {resp}"
    );

    send(&mut writer, "a002 LOGOUT").await;
    read_line(&mut reader).await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_imap_login_failure() {
    let port = start_imap_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "a001 LOGIN testuser wrongpass").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("a001 NO"),
        "LOGIN with wrong creds should return NO: {resp}"
    );

    send(&mut writer, "a002 LOGOUT").await;
    read_line(&mut reader).await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_imap_logout() {
    let port = start_imap_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "a001 LOGOUT").await;
    let bye = read_line(&mut reader).await;
    assert!(bye.starts_with("* BYE"), "LOGOUT should produce BYE: {bye}");
    let ok = read_line(&mut reader).await;
    assert!(
        ok.starts_with("a001 OK"),
        "LOGOUT should produce tagged OK: {ok}"
    );
}

#[tokio::test]
async fn e2e_imap_noop() {
    let port = start_imap_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    send(&mut writer, "a001 NOOP").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("a001 OK"), "NOOP should return OK: {resp}");

    send(&mut writer, "a002 LOGOUT").await;
    read_line(&mut reader).await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_imap_list_requires_auth() {
    let port = start_imap_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    // LIST without authentication
    send(&mut writer, "a001 LIST \"\" \"*\"").await;
    let resp = read_line(&mut reader).await;
    assert!(
        resp.starts_with("a001 NO"),
        "LIST without auth should return NO: {resp}"
    );

    send(&mut writer, "a002 LOGOUT").await;
    read_line(&mut reader).await;
    read_line(&mut reader).await;
}

#[tokio::test]
async fn e2e_imap_list_after_auth() {
    let port = start_imap_server().await;
    let (mut reader, mut writer) = connect(port).await;
    read_line(&mut reader).await;

    // login first
    send(&mut writer, "a001 LOGIN testuser testpass").await;
    let resp = read_line(&mut reader).await;
    assert!(resp.starts_with("a001 OK"));

    // LIST after auth
    send(&mut writer, "a002 LIST \"\" \"*\"").await;
    let list_line = read_line(&mut reader).await;
    assert!(
        list_line.starts_with("* LIST"),
        "LIST should return untagged LIST response: {list_line}"
    );
    assert!(
        list_line.contains("INBOX"),
        "LIST should contain INBOX: {list_line}"
    );

    let ok = read_line(&mut reader).await;
    assert!(ok.starts_with("a002 OK"), "LIST should end with OK: {ok}");

    send(&mut writer, "a003 LOGOUT").await;
    read_line(&mut reader).await;
    read_line(&mut reader).await;
}
