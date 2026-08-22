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
