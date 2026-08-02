//! RCPT TO: forward paths, postmaster, and parameters.

use crate::command::{Command, ForwardPath, Param};
use crate::parse::ParseError;
use crate::parse::parse_command;

// --- RCPT TO edge cases ---

#[test]
fn rcpt_missing_to_keyword_err() {
    // "RCPT FROM:<a@b>" — wrong keyword
    assert!(matches!(
        parse_command("RCPT FROM:<a@b>"),
        Err(ParseError::InvalidSyntax(_))
    ));
}

#[test]
fn rcpt_to_space_before_bracket() {
    assert_eq!(
        parse_command("RCPT TO: <user@example.com>"),
        Ok(Command::RcptTo {
            path: ForwardPath::Path("user@example.com"),
            params: vec![],
        })
    );
}

#[test]
fn rcpt_to_unclosed_bracket_err() {
    assert!(matches!(
        parse_command("RCPT TO:<user@example.com"),
        Err(ParseError::InvalidSyntax(_))
    ));
}

// --- RCPT TO postmaster case insensitivity ---

#[test]
fn rcpt_to_postmaster_lowercase() {
    assert_eq!(
        parse_command("RCPT TO:<postmaster>"),
        Ok(Command::RcptTo {
            path: ForwardPath::Postmaster,
            params: vec![],
        })
    );
}

#[test]
fn rcpt_to_postmaster_uppercase() {
    assert_eq!(
        parse_command("RCPT TO:<POSTMASTER>"),
        Ok(Command::RcptTo {
            path: ForwardPath::Postmaster,
            params: vec![],
        })
    );
}

// --- RCPT TO with params ---

#[test]
fn rcpt_to_with_params() {
    assert_eq!(
        parse_command("RCPT TO:<user@example.com> ORCPT=rfc822;user@example.com"),
        Ok(Command::RcptTo {
            path: ForwardPath::Path("user@example.com"),
            params: vec![Param {
                key: "ORCPT",
                value: "rfc822;user@example.com"
            },],
        })
    );
}

// --- case variants for RCPT ---

#[test]
fn rcpt_to_lowercase() {
    assert_eq!(
        parse_command("rcpt to:<user@test.com>"),
        Ok(Command::RcptTo {
            path: ForwardPath::Path("user@test.com"),
            params: vec![],
        })
    );
}

#[test]
fn rcpt_to_mixed_case() {
    assert_eq!(
        parse_command("Rcpt To:<user@test.com>"),
        Ok(Command::RcptTo {
            path: ForwardPath::Path("user@test.com"),
            params: vec![],
        })
    );
}

// --- RCPT TO postmaster mixed case ---

#[test]
fn rcpt_to_postmaster_mixed_case() {
    assert_eq!(
        parse_command("RCPT TO:<PoStMaStEr>"),
        Ok(Command::RcptTo {
            path: ForwardPath::Postmaster,
            params: vec![],
        })
    );
}
