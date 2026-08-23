//! `AUTH PLAIN`, for relaying through somebody else's submission
//! server.
//!
//! Delivering to an MX needs no credential; submitting *as a user* —
//! which is what a connected Gmail or QQ account is — needs one, and
//! the encoding has a trap in it: the three fields are separated by NUL
//! bytes inside one base64 blob, so a naive `format!` with spaces
//! authenticates as nobody and reads as a wrong password.

use base64::Engine as _;
use mailrs_smtp_client::auth_plain_payload;

#[test]
fn the_three_fields_are_nul_separated() {
    let blob = auth_plain_payload("me@x.com", "hunter2");
    let raw = base64::engine::general_purpose::STANDARD
        .decode(&blob)
        .expect("valid base64");
    assert_eq!(raw, b"\0me@x.com\0hunter2");
}

/// The authorisation identity is empty, not a repeat of the username.
/// Some servers accept both; Gmail refuses the second.
#[test]
fn the_authorisation_identity_is_left_empty() {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(auth_plain_payload("a@b.c", "p"))
        .unwrap();
    assert!(raw.starts_with(b"\0"), "{raw:?}");
}

/// App passwords are generated, and what they generate includes
/// spaces, quotes and backslashes. Base64 carries all of it; nothing
/// here may try to escape or trim.
#[test]
fn a_generated_password_survives_verbatim() {
    for secret in [
        "abcd efgh ijkl mnop",
        "pa\"ss\\word",
        "  leading",
        "trailing  ",
    ] {
        let raw = base64::engine::general_purpose::STANDARD
            .decode(auth_plain_payload("u@h", secret))
            .unwrap();
        let got = raw.split(|b| *b == 0).nth(2).expect("a third field");
        assert_eq!(got, secret.as_bytes(), "{secret:?} did not survive");
    }
}

#[test]
fn non_ascii_is_carried_as_utf8() {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(auth_plain_payload("私@例.jp", "パスワード"))
        .unwrap();
    assert!(raw.ends_with("パスワード".as_bytes()));
}

/// An access token is not a password, and the difference is the whole
/// point: `\x01` separators, a `Bearer` prefix, and two terminators.
/// Sent through `AUTH PLAIN` a token is refused, and the person is
/// told their password is wrong for an account whose credentials are
/// perfectly good.
#[test]
fn an_access_token_is_not_sent_as_a_password() {
    use base64::Engine as _;
    let plain = base64::engine::general_purpose::STANDARD
        .decode(auth_plain_payload("me@gmail.com", "ya29.token"))
        .expect("base64");
    let xoauth = base64::engine::general_purpose::STANDARD
        .decode(mailrs_smtp_client::auth_xoauth2_payload(
            "me@gmail.com",
            "ya29.token",
        ))
        .expect("base64");
    assert_ne!(plain, xoauth, "the two verbs would carry the same bytes");
    assert_eq!(
        xoauth, b"user=me@gmail.com\x01auth=Bearer ya29.token\x01\x01",
        "the payload is not what a provider expects"
    );
    assert!(
        !xoauth.contains(&0),
        "a NUL separator here is the AUTH PLAIN shape, which is refused"
    );
}

/// Every byte of the token survives, including the ones that look like
/// separators in the other verb.
#[test]
fn a_token_with_awkward_bytes_arrives_whole() {
    use base64::Engine as _;
    let token = "abc.def-ghi_jkl=";
    let raw = base64::engine::general_purpose::STANDARD
        .decode(mailrs_smtp_client::auth_xoauth2_payload("u@h", token))
        .expect("base64");
    let text = String::from_utf8(raw).expect("utf8");
    assert!(text.contains(token), "the token was mangled: {text:?}");
    assert!(text.ends_with("\u{1}\u{1}"), "missing the two terminators");
}
