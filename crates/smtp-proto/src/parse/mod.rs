//! Wire-format command-line parser for SMTP.

use crate::command::{AuthMechanism, Command, ForwardPath, Param, ReversePath};

/// Error returned by [`parse_command`].
#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    /// Verb did not match any known SMTP command.
    UnknownCommand,
    /// Verb is known but the arguments don't match the expected syntax.
    InvalidSyntax(String),
    /// Input ended before a complete command was read.
    Incomplete,
}

/// Parse a single SMTP command line (the line passed in must NOT include the
/// trailing CRLF).
///
/// The returned [`Command<'_>`] borrows from `input`, so `input` must
/// outlive the command.
pub fn parse_command(input: &str) -> Result<Command<'_>, ParseError> {
    if input.is_empty() {
        return Err(ParseError::Incomplete);
    }

    let (verb, args) = match input.find(' ') {
        Some(pos) => (&input[..pos], input[pos + 1..].trim_start()),
        None => (input, ""),
    };

    // ASCII-uppercase verb into a stack buffer. Longest SMTP verb
    // we accept is "STARTTLS" (8); 16 gives headroom for any
    // future ESMTP extension verb. Killing the `to_ascii_uppercase()`
    // String allocation is the dominant DATA-path saving (no args).
    let verb_bytes = verb.as_bytes();
    if verb_bytes.len() > 16 {
        return Err(ParseError::UnknownCommand);
    }
    let mut verb_upper = [0u8; 16];
    for (i, &b) in verb_bytes.iter().enumerate() {
        verb_upper[i] = b.to_ascii_uppercase();
    }
    let verb_upper = &verb_upper[..verb_bytes.len()];

    match verb_upper {
        b"EHLO" => parse_ehlo(args),
        b"HELO" => parse_helo(args),
        b"MAIL" => parse_mail_from(args),
        b"RCPT" => parse_rcpt_to(args),
        b"DATA" => Ok(Command::Data),
        b"RSET" => Ok(Command::Rset),
        b"QUIT" => Ok(Command::Quit),
        b"NOOP" => Ok(Command::Noop(if args.is_empty() {
            None
        } else {
            Some(&input[verb.len() + 1..])
        })),
        b"VRFY" => {
            if args.is_empty() {
                Err(ParseError::InvalidSyntax("VRFY requires argument".into()))
            } else {
                Ok(Command::Vrfy(args))
            }
        }
        b"HELP" => Ok(Command::Help(if args.is_empty() {
            None
        } else {
            Some(args)
        })),
        b"STARTTLS" => parse_starttls(args),
        b"AUTH" => parse_auth(args, input),
        _ => Err(ParseError::UnknownCommand),
    }
}

fn parse_ehlo(args: &str) -> Result<Command<'_>, ParseError> {
    if args.is_empty() {
        return Err(ParseError::InvalidSyntax("EHLO requires domain".into()));
    }
    Ok(Command::Ehlo(args))
}

fn parse_helo(args: &str) -> Result<Command<'_>, ParseError> {
    if args.is_empty() {
        return Err(ParseError::InvalidSyntax("HELO requires domain".into()));
    }
    Ok(Command::Helo(args))
}

fn parse_mail_from(args: &str) -> Result<Command<'_>, ParseError> {
    // expect "FROM:" prefix (case-insensitive)
    let upper = args.to_ascii_uppercase();
    if !upper.starts_with("FROM:") {
        return Err(ParseError::InvalidSyntax("expected FROM:".into()));
    }
    let after_from = args[5..].trim_start();
    parse_reverse_path_and_params(after_from)
}

fn parse_rcpt_to(args: &str) -> Result<Command<'_>, ParseError> {
    let upper = args.to_ascii_uppercase();
    if !upper.starts_with("TO:") {
        return Err(ParseError::InvalidSyntax("expected TO:".into()));
    }
    let after_to = args[3..].trim_start();
    parse_forward_path_and_params(after_to)
}

fn parse_reverse_path_and_params(input: &str) -> Result<Command<'_>, ParseError> {
    let (path_str, rest) = extract_angle_addr(input)?;

    if path_str.is_empty() {
        let params = parse_params(rest)?;
        return Ok(Command::MailFrom {
            path: ReversePath::Null,
            params,
        });
    }

    // strip source route if present (e.g. @a.com:user@b.com -> user@b.com)
    let addr = strip_source_route(path_str);

    let params = parse_params(rest)?;
    Ok(Command::MailFrom {
        path: ReversePath::Path(addr),
        params,
    })
}

fn parse_forward_path_and_params(input: &str) -> Result<Command<'_>, ParseError> {
    let (path_str, rest) = extract_angle_addr(input)?;

    if path_str.eq_ignore_ascii_case("Postmaster") {
        let params = parse_params(rest)?;
        return Ok(Command::RcptTo {
            path: ForwardPath::Postmaster,
            params,
        });
    }

    let params = parse_params(rest)?;
    Ok(Command::RcptTo {
        path: ForwardPath::Path(path_str),
        params,
    })
}

/// extract content between < and >, returning (content, rest_after_close)
fn extract_angle_addr(input: &str) -> Result<(&str, &str), ParseError> {
    let input = input.trim_start();
    if !input.starts_with('<') {
        return Err(ParseError::InvalidSyntax("expected '<' in path".into()));
    }

    // handle quoted strings inside the angle brackets
    let after_open = &input[1..];
    let mut in_quote = false;
    let mut escape = false;

    for (i, ch) in after_open.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        match ch {
            '\\' if in_quote => escape = true,
            '"' => in_quote = !in_quote,
            '>' if !in_quote => {
                let content = &after_open[..i];
                let rest = &after_open[i + 1..];
                return Ok((content, rest.trim_start()));
            }
            _ => {}
        }
    }

    Err(ParseError::InvalidSyntax("missing '>' in path".into()))
}

/// strip obsolete source routes: @a.com,@b.com:user@c.com -> user@c.com
fn strip_source_route(path: &str) -> &str {
    if path.starts_with('@')
        && let Some(colon_pos) = path.find(':')
    {
        return &path[colon_pos + 1..];
    }
    path
}

fn parse_params(input: &str) -> Result<Vec<Param<'_>>, ParseError> {
    if input.is_empty() {
        return Ok(vec![]);
    }

    let mut params = Vec::new();
    for part in input.split_whitespace() {
        if let Some(eq_pos) = part.find('=') {
            params.push(Param {
                key: &part[..eq_pos],
                value: &part[eq_pos + 1..],
            });
        } else {
            params.push(Param {
                key: part,
                value: "",
            });
        }
    }
    Ok(params)
}

fn parse_starttls(args: &str) -> Result<Command<'_>, ParseError> {
    if !args.is_empty() {
        return Err(ParseError::InvalidSyntax(
            "STARTTLS takes no arguments".into(),
        ));
    }
    Ok(Command::StartTls)
}

fn parse_auth<'a>(args: &'a str, _input: &'a str) -> Result<Command<'a>, ParseError> {
    if args.is_empty() {
        return Err(ParseError::InvalidSyntax("AUTH requires mechanism".into()));
    }

    let (mech_str, rest) = match args.find(' ') {
        Some(pos) => (&args[..pos], args[pos + 1..].trim_start()),
        None => (args, ""),
    };

    // Same stack-buffer pattern as the main verb match — AUTH
    // mechanisms are short ASCII keywords (PLAIN / LOGIN /
    // CRAM-MD5 / GSSAPI etc.), well under 16 bytes.
    let mech_bytes = mech_str.as_bytes();
    let mut mech_upper = [0u8; 16];
    if mech_bytes.len() > 16 {
        return Err(ParseError::InvalidSyntax(format!(
            "unsupported AUTH mechanism: {mech_str}"
        )));
    }
    for (i, &b) in mech_bytes.iter().enumerate() {
        mech_upper[i] = b.to_ascii_uppercase();
    }
    let mechanism = match &mech_upper[..mech_bytes.len()] {
        b"PLAIN" => AuthMechanism::Plain,
        b"LOGIN" => AuthMechanism::Login,
        _ => {
            return Err(ParseError::InvalidSyntax(format!(
                "unsupported AUTH mechanism: {mech_str}"
            )));
        }
    };

    let initial_response = if rest.is_empty() { None } else { Some(rest) };

    Ok(Command::Auth {
        mechanism,
        initial_response,
    })
}

#[cfg(test)]
mod tests;
