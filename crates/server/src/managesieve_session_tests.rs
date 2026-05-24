//! Tests for `managesieve_session` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

fn make_session() -> ManageSieveSession {
    ManageSieveSession::new(Arc::new(UserStore::empty()))
}

#[test]
fn greeting_contains_implementation() {
    let session = make_session();
    let greeting = session.greeting();
    assert!(greeting.contains("IMPLEMENTATION"));
    assert!(greeting.contains("mailrs"));
}

#[tokio::test]
async fn capability_response() {
    let mut session = make_session();
    let resp = session.handle_line("CAPABILITY").await;
    let joined = resp.join("");
    assert!(joined.contains("SIEVE"));
    assert!(joined.contains("SASL"));
    assert!(joined.contains("OK"));
}

#[tokio::test]
async fn authenticate_fails_without_credentials() {
    let mut session = make_session();
    let resp = session.handle_line("AUTHENTICATE \"PLAIN\"").await;
    assert!(resp[0].starts_with("NO"));
}

#[tokio::test]
async fn authenticate_fails_bad_mechanism() {
    let mut session = make_session();
    let resp = session.handle_line("AUTHENTICATE \"CRAM-MD5\" dGVzdA==").await;
    assert!(resp[0].starts_with("NO"));
    assert!(resp[0].contains("unsupported"));
}

#[tokio::test]
async fn authenticate_fails_invalid_base64() {
    let mut session = make_session();
    let resp = session.handle_line("AUTHENTICATE \"PLAIN\" !!!invalid!!!").await;
    assert!(resp[0].starts_with("NO"));
}

#[tokio::test]
async fn authenticate_fails_bad_credentials() {
    let users = Arc::new(UserStore::from_plain_passwords(vec![
        ("alice@example.com".into(), "secret".into()),
    ]));
    let mut session = ManageSieveSession::new(users);
    // encode \0alice@example.com\0wrong
    let cred = base64_encode(b"\0alice@example.com\0wrong");
    let resp = session
        .handle_line(&format!("AUTHENTICATE \"PLAIN\" \"{cred}\""))
        .await;
    assert!(resp[0].starts_with("NO"));
}

#[tokio::test]
async fn authenticate_succeeds() {
    let users = Arc::new(UserStore::from_plain_passwords(vec![
        ("alice@example.com".into(), "secret".into()),
    ]));
    let mut session = ManageSieveSession::new(users);
    let cred = base64_encode(b"\0alice@example.com\0secret");
    let resp = session
        .handle_line(&format!("AUTHENTICATE \"PLAIN\" \"{cred}\""))
        .await;
    assert!(resp[0].starts_with("OK"));
    assert!(resp[0].contains("authenticated"));
}

#[tokio::test]
async fn listscripts_requires_auth() {
    let mut session = make_session();
    let resp = session.handle_line("LISTSCRIPTS").await;
    assert!(resp[0].starts_with("NO"));
}

#[tokio::test]
async fn getscript_requires_auth() {
    let mut session = make_session();
    let resp = session.handle_line("GETSCRIPT \"default\"").await;
    assert!(resp[0].starts_with("NO"));
}

#[tokio::test]
async fn logout_response() {
    let mut session = make_session();
    let resp = session.handle_line("LOGOUT").await;
    assert!(resp[0].contains("Bye"));
    assert!(session.should_close(&resp));
}

#[tokio::test]
async fn unknown_command() {
    let mut session = make_session();
    let resp = session.handle_line("FOOBAR").await;
    assert!(resp[0].starts_with("NO"));
}

#[tokio::test]
async fn setactive_requires_auth() {
    let mut session = make_session();
    let resp = session.handle_line("SETACTIVE \"default\"").await;
    assert!(resp[0].starts_with("NO"));
}

#[tokio::test]
async fn havespace_always_ok() {
    let mut session = make_session();
    let resp = session.handle_line("HAVESPACE \"test\" 1024").await;
    assert_eq!(resp[0], "OK\r\n");
}

#[test]
fn unquote_removes_quotes() {
    assert_eq!(unquote("\"hello\""), "hello");
    assert_eq!(unquote("hello"), "hello");
    assert_eq!(unquote("\"\""), "");
    assert_eq!(unquote("\""), "\"");
}

#[test]
fn base64_decode_valid() {
    let decoded = base64_decode("dGVzdA==");
    assert_eq!(decoded, Some(b"test".to_vec()));
}

#[test]
fn base64_decode_invalid() {
    assert!(base64_decode("!!!").is_none());
}

fn base64_encode(input: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(input)
}
