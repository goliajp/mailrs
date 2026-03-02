use crate::command::{AuthMechanism, Command, ForwardPath, Param, ReversePath};
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
    assert_eq!(
        parse_command("HELP MAIL"),
        Ok(Command::Help(Some("MAIL")))
    );
}

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
