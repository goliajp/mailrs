//! What the parser rejects, and with which error.

use crate::command::{Command, ForwardPath, ReversePath};
use crate::parse::ParseError;
use crate::parse::parse_command;

// --- parse failures ---

#[test]
fn unknown_command() {
    assert!(parse_command("XUNK arg").is_err());
}

#[test]
fn mail_from_no_brackets() {
    assert!(parse_command("MAIL FROM:user@example.com").is_err());
}

#[test]
fn empty_line() {
    assert!(parse_command("").is_err());
}

#[test]
fn mail_from_missing_addr() {
    assert!(parse_command("MAIL FROM:").is_err());
}

// --- edge cases ---

#[test]
fn mail_from_quoted_local() {
    assert_eq!(
        parse_command(r#"MAIL FROM:<"user name"@example.com>"#),
        Ok(Command::MailFrom {
            path: ReversePath::Path(r#""user name"@example.com"#),
            params: vec![],
        })
    );
}

#[test]
fn mail_from_source_route() {
    // source routes are obsolete but must be accepted; we strip the route
    assert_eq!(
        parse_command("MAIL FROM:<@a.com:user@b.com>"),
        Ok(Command::MailFrom {
            path: ReversePath::Path("user@b.com"),
            params: vec![],
        })
    );
}

#[test]
fn rcpt_to_postmaster_domain() {
    // postmaster@domain is a regular path, not the special <Postmaster>
    assert_eq!(
        parse_command("RCPT TO:<Postmaster@example.com>"),
        Ok(Command::RcptTo {
            path: ForwardPath::Path("Postmaster@example.com"),
            params: vec![],
        })
    );
}

// --- VRFY error case ---

#[test]
fn vrfy_no_arg_err() {
    assert!(matches!(
        parse_command("VRFY"),
        Err(ParseError::InvalidSyntax(_))
    ));
}

// --- whitespace-only input ---

#[test]
fn whitespace_only_input() {
    // leading whitespace means verb is empty after no-space split
    assert!(parse_command("   ").is_err());
}

// --- parse error type checks ---

#[test]
fn incomplete_error_on_empty() {
    assert_eq!(parse_command(""), Err(ParseError::Incomplete));
}

#[test]
fn unknown_command_error_variant() {
    assert_eq!(parse_command("XYZZY foo"), Err(ParseError::UnknownCommand));
}

// --- only verb, no space, not a known no-arg command ---

#[test]
fn mail_alone_no_args_err() {
    assert!(matches!(
        parse_command("MAIL"),
        Err(ParseError::InvalidSyntax(_))
    ));
}

#[test]
fn rcpt_alone_no_args_err() {
    assert!(matches!(
        parse_command("RCPT"),
        Err(ParseError::InvalidSyntax(_))
    ));
}
