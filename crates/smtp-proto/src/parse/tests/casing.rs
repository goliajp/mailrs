//! Verb and keyword case handling.

use crate::command::{Command, ReversePath};
use crate::parse::ParseError;
use crate::parse::parse_command;

// --- case insensitivity ---

#[test]
fn mail_from_lowercase() {
    assert_eq!(
        parse_command("mail from:<a@b>"),
        Ok(Command::MailFrom {
            path: ReversePath::Path("a@b"),
            params: vec![],
        })
    );
}

#[test]
fn mail_from_mixed() {
    assert_eq!(
        parse_command("Mail FROM:<a@b>"),
        Ok(Command::MailFrom {
            path: ReversePath::Path("a@b"),
            params: vec![],
        })
    );
}

#[test]
fn ehlo_mixed() {
    assert_eq!(
        parse_command("eHlO example.com"),
        Ok(Command::Ehlo("example.com"))
    );
}

// --- EHLO/HELO missing argument errors ---

#[test]
fn ehlo_no_domain_err() {
    assert!(matches!(
        parse_command("EHLO"),
        Err(ParseError::InvalidSyntax(_))
    ));
}

#[test]
fn helo_no_domain_err() {
    assert!(matches!(
        parse_command("HELO"),
        Err(ParseError::InvalidSyntax(_))
    ));
}

// --- HELO lowercase ---

#[test]
fn helo_lowercase() {
    assert_eq!(
        parse_command("helo example.com"),
        Ok(Command::Helo("example.com"))
    );
}

// --- DATA / RSET / QUIT case variants ---

#[test]
fn data_lowercase() {
    assert_eq!(parse_command("data"), Ok(Command::Data));
}

#[test]
fn rset_lowercase() {
    assert_eq!(parse_command("rset"), Ok(Command::Rset));
}

#[test]
fn quit_lowercase() {
    assert_eq!(parse_command("quit"), Ok(Command::Quit));
}

// --- NOOP lowercase ---

#[test]
fn noop_lowercase() {
    assert_eq!(parse_command("noop"), Ok(Command::Noop(None)));
}

// --- HELP lowercase ---

#[test]
fn help_lowercase() {
    assert_eq!(parse_command("help"), Ok(Command::Help(None)));
}
