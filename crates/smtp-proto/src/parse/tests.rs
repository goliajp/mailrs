use crate::command::{AuthMechanism, Command, ForwardPath, Param, ReversePath};
use crate::parse::parse_command;

use crate::parse::ParseError;

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

// --- MAIL FROM edge cases ---

#[test]
fn mail_missing_from_keyword_err() {
    // "MAIL TO:<a@b>" — wrong keyword
    assert!(matches!(
        parse_command("MAIL TO:<a@b>"),
        Err(ParseError::InvalidSyntax(_))
    ));
}

#[test]
fn mail_from_unclosed_bracket_err() {
    assert!(matches!(
        parse_command("MAIL FROM:<user@example.com"),
        Err(ParseError::InvalidSyntax(_))
    ));
}

#[test]
fn mail_from_param_no_value() {
    // param without '=' is stored with empty value
    let result = parse_command("MAIL FROM:<a@b> FLAGONLY");
    assert!(result.is_ok());
    if let Ok(Command::MailFrom { params, .. }) = result {
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].key, "FLAGONLY");
        assert_eq!(params[0].value, "");
    }
}

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

// --- VRFY error case ---

#[test]
fn vrfy_no_arg_err() {
    assert!(matches!(
        parse_command("VRFY"),
        Err(ParseError::InvalidSyntax(_))
    ));
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

// --- NOOP with multi-word arg ---

#[test]
fn noop_with_multi_word_arg() {
    // everything after "NOOP " is the argument
    assert_eq!(
        parse_command("NOOP hello world"),
        Ok(Command::Noop(Some("hello world")))
    );
}

// --- MAIL FROM source route with multiple hops ---

#[test]
fn mail_from_source_route_multi_hop() {
    // @a.com,@b.com:user@c.com — strip up to colon
    assert_eq!(
        parse_command("MAIL FROM:<@a.com,@b.com:user@c.com>"),
        Ok(Command::MailFrom {
            path: ReversePath::Path("user@c.com"),
            params: vec![],
        })
    );
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

// --- MAIL FROM with multiple params ---

#[test]
fn mail_from_three_params() {
    assert_eq!(
        parse_command("MAIL FROM:<a@b> SIZE=1024 BODY=8BITMIME SMTPUTF8"),
        Ok(Command::MailFrom {
            path: ReversePath::Path("a@b"),
            params: vec![
                Param {
                    key: "SIZE",
                    value: "1024"
                },
                Param {
                    key: "BODY",
                    value: "8BITMIME"
                },
                Param {
                    key: "SMTPUTF8",
                    value: ""
                },
            ],
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

// --- quoted string with escaped characters in MAIL FROM ---

#[test]
fn mail_from_quoted_with_backslash_escape() {
    assert_eq!(
        parse_command(r#"MAIL FROM:<"user\"name"@example.com>"#),
        Ok(Command::MailFrom {
            path: ReversePath::Path(r#""user\"name"@example.com"#),
            params: vec![],
        })
    );
}

// --- angle bracket with quoted greater-than ---

#[test]
fn mail_from_quoted_angle_bracket() {
    assert_eq!(
        parse_command(r#"MAIL FROM:<"a>b"@example.com>"#),
        Ok(Command::MailFrom {
            path: ReversePath::Path(r#""a>b"@example.com"#),
            params: vec![],
        })
    );
}

// --- tab and extra spaces in MAIL FROM ---

#[test]
fn mail_from_extra_whitespace_after_from() {
    assert_eq!(
        parse_command("MAIL FROM:   <user@test.com>"),
        Ok(Command::MailFrom {
            path: ReversePath::Path("user@test.com"),
            params: vec![],
        })
    );
}

// --- RCPT TO with extra space between TO: and angle bracket ---

#[test]
fn rcpt_to_multiple_spaces_before_angle() {
    assert_eq!(
        parse_command("RCPT TO:   <user@test.com>"),
        Ok(Command::RcptTo {
            path: ForwardPath::Path("user@test.com"),
            params: vec![],
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

// --- VRFY with email address arg ---

#[test]
fn vrfy_with_email_address() {
    assert_eq!(
        parse_command("VRFY user@example.com"),
        Ok(Command::Vrfy("user@example.com"))
    );
}

// --- HELP lowercase ---

#[test]
fn help_lowercase() {
    assert_eq!(parse_command("help"), Ok(Command::Help(None)));
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

// --- source route with no colon (just @domain) ---

#[test]
fn mail_from_at_prefix_without_colon_not_stripped() {
    // "@domain" without colon — no source route stripping occurs
    assert_eq!(
        parse_command("MAIL FROM:<@domain>"),
        Ok(Command::MailFrom {
            path: ReversePath::Path("@domain"),
            params: vec![],
        })
    );
}

// --- MAIL FROM null sender with params ---

#[test]
fn mail_from_null_with_size_param() {
    assert_eq!(
        parse_command("MAIL FROM:<> SIZE=0"),
        Ok(Command::MailFrom {
            path: ReversePath::Null,
            params: vec![Param {
                key: "SIZE",
                value: "0"
            }],
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

// --- MAIL FROM with empty angle brackets and space padding ---

#[test]
fn mail_from_null_with_spaces() {
    assert_eq!(
        parse_command("MAIL FROM: <>"),
        Ok(Command::MailFrom {
            path: ReversePath::Null,
            params: vec![],
        })
    );
}
