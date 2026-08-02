//! STARTTLS and AUTH.

use crate::command::{AuthMechanism, Command};
use crate::parse::ParseError;
use crate::parse::parse_command;

// --- STARTTLS + AUTH parsing ---

#[test]
fn starttls_cmd() {
    assert_eq!(parse_command("STARTTLS"), Ok(Command::StartTls));
}

#[test]
fn starttls_with_args_err() {
    assert!(parse_command("STARTTLS extra").is_err());
}

#[test]
fn auth_plain_with_initial() {
    assert_eq!(
        parse_command("AUTH PLAIN dGVzdAB0ZXN0AHBhc3M="),
        Ok(Command::Auth {
            mechanism: AuthMechanism::Plain,
            initial_response: Some("dGVzdAB0ZXN0AHBhc3M="),
        })
    );
}

#[test]
fn auth_plain_no_initial() {
    assert_eq!(
        parse_command("AUTH PLAIN"),
        Ok(Command::Auth {
            mechanism: AuthMechanism::Plain,
            initial_response: None,
        })
    );
}

#[test]
fn auth_login() {
    assert_eq!(
        parse_command("AUTH LOGIN"),
        Ok(Command::Auth {
            mechanism: AuthMechanism::Login,
            initial_response: None,
        })
    );
}

#[test]
fn auth_unknown_mechanism() {
    assert!(parse_command("AUTH CRAM-MD5").is_err());
}

#[test]
fn auth_case_insensitive() {
    assert_eq!(
        parse_command("auth plain"),
        Ok(Command::Auth {
            mechanism: AuthMechanism::Plain,
            initial_response: None,
        })
    );
}

// --- STARTTLS extra whitespace ---

#[test]
fn starttls_lowercase() {
    assert_eq!(parse_command("starttls"), Ok(Command::StartTls));
}

// --- AUTH edge cases ---

#[test]
fn auth_no_mechanism_err() {
    assert!(matches!(
        parse_command("AUTH"),
        Err(ParseError::InvalidSyntax(_))
    ));
}

#[test]
fn auth_login_with_initial_response() {
    // AUTH LOGIN with an initial response (unusual but parseable)
    assert_eq!(
        parse_command("AUTH LOGIN dXNlcg=="),
        Ok(Command::Auth {
            mechanism: AuthMechanism::Login,
            initial_response: Some("dXNlcg=="),
        })
    );
}

// --- AUTH LOGIN case insensitive mechanism ---

#[test]
fn auth_login_uppercase_mechanism() {
    assert_eq!(
        parse_command("AUTH LOGIN"),
        Ok(Command::Auth {
            mechanism: AuthMechanism::Login,
            initial_response: None,
        })
    );
}

#[test]
fn auth_login_mixed_case_mechanism() {
    assert_eq!(
        parse_command("AUTH Login"),
        Ok(Command::Auth {
            mechanism: AuthMechanism::Login,
            initial_response: None,
        })
    );
}
