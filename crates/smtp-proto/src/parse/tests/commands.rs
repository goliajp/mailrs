//! The rest of the verb surface.

use crate::command::{Command, ForwardPath, Param, ReversePath};
use crate::parse::parse_command;

// --- basic commands ---

#[test]
fn ehlo_domain() {
    assert_eq!(
        parse_command("EHLO mail.example.com"),
        Ok(Command::Ehlo("mail.example.com"))
    );
}

#[test]
fn ehlo_ipv4() {
    assert_eq!(
        parse_command("EHLO [192.0.2.1]"),
        Ok(Command::Ehlo("[192.0.2.1]"))
    );
}

#[test]
fn ehlo_ipv6() {
    assert_eq!(
        parse_command("EHLO [IPv6:2001:db8::1]"),
        Ok(Command::Ehlo("[IPv6:2001:db8::1]"))
    );
}

#[test]
fn helo_domain() {
    assert_eq!(
        parse_command("HELO mail.example.com"),
        Ok(Command::Helo("mail.example.com"))
    );
}

#[test]
fn mail_from_simple() {
    assert_eq!(
        parse_command("MAIL FROM:<user@example.com>"),
        Ok(Command::MailFrom {
            path: ReversePath::Path("user@example.com"),
            params: vec![],
        })
    );
}

#[test]
fn mail_from_null() {
    assert_eq!(
        parse_command("MAIL FROM:<>"),
        Ok(Command::MailFrom {
            path: ReversePath::Null,
            params: vec![],
        })
    );
}

#[test]
fn mail_from_space() {
    assert_eq!(
        parse_command("MAIL FROM: <user@example.com>"),
        Ok(Command::MailFrom {
            path: ReversePath::Path("user@example.com"),
            params: vec![],
        })
    );
}

#[test]
fn mail_from_params() {
    assert_eq!(
        parse_command("MAIL FROM:<a@b> SIZE=1024 BODY=8BITMIME"),
        Ok(Command::MailFrom {
            path: ReversePath::Path("a@b"),
            params: vec![
                Param {
                    key: "SIZE",
                    value: "1024",
                },
                Param {
                    key: "BODY",
                    value: "8BITMIME",
                },
            ],
        })
    );
}

#[test]
fn rcpt_to_simple() {
    assert_eq!(
        parse_command("RCPT TO:<user@example.com>"),
        Ok(Command::RcptTo {
            path: ForwardPath::Path("user@example.com"),
            params: vec![],
        })
    );
}

#[test]
fn rcpt_to_postmaster() {
    assert_eq!(
        parse_command("RCPT TO:<Postmaster>"),
        Ok(Command::RcptTo {
            path: ForwardPath::Postmaster,
            params: vec![],
        })
    );
}

#[test]
fn data_cmd() {
    assert_eq!(parse_command("DATA"), Ok(Command::Data));
}

#[test]
fn rset_cmd() {
    assert_eq!(parse_command("RSET"), Ok(Command::Rset));
}

#[test]
fn quit_cmd() {
    assert_eq!(parse_command("QUIT"), Ok(Command::Quit));
}

#[test]
fn noop_bare() {
    assert_eq!(parse_command("NOOP"), Ok(Command::Noop(None)));
}

#[test]
fn noop_with_arg() {
    assert_eq!(
        parse_command("NOOP hello"),
        Ok(Command::Noop(Some("hello")))
    );
}

#[test]
fn vrfy_cmd() {
    assert_eq!(parse_command("VRFY user"), Ok(Command::Vrfy("user")));
}

#[test]
fn help_bare() {
    assert_eq!(parse_command("HELP"), Ok(Command::Help(None)));
}

#[test]
fn help_with_arg() {
    assert_eq!(parse_command("HELP MAIL"), Ok(Command::Help(Some("MAIL"))));
}

// --- NOOP with multi-word arg ---

#[test]
fn noop_with_multi_word_arg() {
    // everything after "NOOP " is the argument
    assert_eq!(
        parse_command("NOOP hello world"),
        Ok(Command::Noop(Some("hello world")))
    );
}

// --- pipelining: multiple commands parsed independently ---

#[test]
fn pipelining_commands_parsed_individually() {
    // pipelining means multiple commands sent at once; each is parsed separately
    let cmds = ["MAIL FROM:<a@b.com>", "RCPT TO:<c@d.com>", "DATA"];
    let results: Vec<_> = cmds.iter().map(|c| parse_command(c)).collect();
    assert!(matches!(results[0], Ok(Command::MailFrom { .. })));
    assert!(matches!(results[1], Ok(Command::RcptTo { .. })));
    assert!(matches!(results[2], Ok(Command::Data)));
}

// --- VRFY with email address arg ---

#[test]
fn vrfy_with_email_address() {
    assert_eq!(
        parse_command("VRFY user@example.com"),
        Ok(Command::Vrfy("user@example.com"))
    );
}
