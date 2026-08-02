//! MAIL FROM: reverse paths, source routes, quoting, and parameters.

use crate::command::{Command, ForwardPath, Param, ReversePath};
use crate::parse::ParseError;
use crate::parse::parse_command;

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
