/// IMAP command variants
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImapCommand {
    Capability,
    Login { username: String, password: String },
    Logout,
    List { reference: String, pattern: String },
    Select { mailbox: String },
    Examine { mailbox: String },
    Fetch { sequence: String, attributes: String },
    Store { sequence: String, action: String, flags: String },
    Search { criteria: String },
    Expunge,
    Noop,
    Close,
    Idle,
    Append { mailbox: String, flags: Option<String>, literal_size: u32 },
    Copy { sequence: String, mailbox: String },
    Move { sequence: String, mailbox: String },
    Uid { subcommand: Box<ImapCommand> },
    GetQuota { quotaroot: String },
    GetQuotaRoot { mailbox: String },
}

/// tagged IMAP command
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedCommand {
    pub tag: String,
    pub command: ImapCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    EmptyInput,
    MissingTag,
    UnknownCommand(String),
    MissingArgument(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::EmptyInput => write!(f, "empty input"),
            ParseError::MissingTag => write!(f, "missing tag"),
            ParseError::UnknownCommand(cmd) => write!(f, "unknown command: {cmd}"),
            ParseError::MissingArgument(arg) => write!(f, "missing argument: {arg}"),
        }
    }
}

/// parse a single IMAP command line into a TaggedCommand
pub fn parse_command(line: &str) -> Result<TaggedCommand, ParseError> {
    let line = line.trim();
    if line.is_empty() {
        return Err(ParseError::EmptyInput);
    }

    let (tag, rest) = line.split_once(' ').ok_or(ParseError::MissingTag)?;
    let tag = tag.to_string();

    let (cmd_word, args) = match rest.split_once(' ') {
        Some((c, a)) => (c, a),
        None => (rest, ""),
    };

    let command = match cmd_word.to_uppercase().as_str() {
        "CAPABILITY" => ImapCommand::Capability,
        "LOGIN" => {
            let (username, password) = parse_login_args(args)?;
            ImapCommand::Login { username, password }
        }
        "LOGOUT" => ImapCommand::Logout,
        "LIST" => {
            let (reference, pattern) = parse_list_args(args)?;
            ImapCommand::List { reference, pattern }
        }
        "SELECT" => ImapCommand::Select {
            mailbox: unquote(args.trim()),
        },
        "EXAMINE" => ImapCommand::Examine {
            mailbox: unquote(args.trim()),
        },
        "FETCH" => {
            let (seq, attrs) = args
                .split_once(' ')
                .ok_or(ParseError::MissingArgument("fetch attributes".into()))?;
            ImapCommand::Fetch {
                sequence: seq.to_string(),
                attributes: attrs.to_string(),
            }
        }
        "STORE" => {
            let parts: Vec<&str> = args.splitn(3, ' ').collect();
            if parts.len() < 3 {
                return Err(ParseError::MissingArgument("store flags".into()));
            }
            ImapCommand::Store {
                sequence: parts[0].to_string(),
                action: parts[1].to_string(),
                flags: parts[2].to_string(),
            }
        }
        "SEARCH" => ImapCommand::Search {
            criteria: args.to_string(),
        },
        "EXPUNGE" => ImapCommand::Expunge,
        "NOOP" => ImapCommand::Noop,
        "CLOSE" => ImapCommand::Close,
        "IDLE" => ImapCommand::Idle,
        "APPEND" => {
            parse_append_args(args)?
        }
        "COPY" => {
            let (seq, mb) = args
                .split_once(' ')
                .ok_or(ParseError::MissingArgument("copy mailbox".into()))?;
            ImapCommand::Copy {
                sequence: seq.to_string(),
                mailbox: unquote(mb.trim()),
            }
        }
        "MOVE" => {
            let (seq, mb) = args
                .split_once(' ')
                .ok_or(ParseError::MissingArgument("move mailbox".into()))?;
            ImapCommand::Move {
                sequence: seq.to_string(),
                mailbox: unquote(mb.trim()),
            }
        }
        "GETQUOTA" => ImapCommand::GetQuota {
            quotaroot: unquote(args.trim()),
        },
        "GETQUOTAROOT" => ImapCommand::GetQuotaRoot {
            mailbox: unquote(args.trim()),
        },
        "UID" => {
            // parse the rest as a sub-command with a dummy tag
            let sub_line = format!("_ {args}");
            let sub = parse_command(&sub_line)?;
            ImapCommand::Uid {
                subcommand: Box::new(sub.command),
            }
        }
        other => return Err(ParseError::UnknownCommand(other.to_string())),
    };

    Ok(TaggedCommand { tag, command })
}

/// parse LOGIN username password (supports quoted strings)
fn parse_login_args(args: &str) -> Result<(String, String), ParseError> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    for ch in args.chars() {
        match ch {
            '"' => in_quote = !in_quote,
            ' ' if !in_quote => {
                if !current.is_empty() {
                    parts.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }

    if parts.len() < 2 {
        return Err(ParseError::MissingArgument("username and password".into()));
    }
    Ok((parts[0].clone(), parts[1].clone()))
}

/// parse LIST reference pattern (supports quoted strings)
fn parse_list_args(args: &str) -> Result<(String, String), ParseError> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;

    for ch in args.chars() {
        match ch {
            '"' => in_quote = !in_quote,
            ' ' if !in_quote => {
                parts.push(current.clone());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    parts.push(current);

    if parts.len() < 2 {
        return Err(ParseError::MissingArgument("reference and pattern".into()));
    }
    Ok((parts[0].clone(), parts[1].clone()))
}

/// parse APPEND args: mailbox [flags] {literal_size}
/// e.g.: INBOX (\Seen) {310} or "Drafts" {100}
fn parse_append_args(args: &str) -> Result<ImapCommand, ParseError> {
    let args = args.trim();

    // find the literal size at the end: {N}
    let literal_start = args.rfind('{')
        .ok_or(ParseError::MissingArgument("literal size".into()))?;
    let literal_end = args.rfind('}')
        .ok_or(ParseError::MissingArgument("literal size".into()))?;
    if literal_end <= literal_start {
        return Err(ParseError::MissingArgument("literal size".into()));
    }
    let size_str = &args[literal_start + 1..literal_end];
    let literal_size: u32 = size_str.parse()
        .map_err(|_| ParseError::MissingArgument(format!("invalid literal size: {size_str}")))?;

    let before_literal = args[..literal_start].trim();

    // first token is the mailbox name
    let (mailbox, rest) = if before_literal.starts_with('"') {
        // quoted mailbox
        let end = before_literal[1..].find('"')
            .ok_or(ParseError::MissingArgument("mailbox".into()))?;
        let mb = before_literal[1..=end].to_string();
        let rest = before_literal[end + 2..].trim();
        (mb, rest)
    } else {
        match before_literal.split_once(' ') {
            Some((mb, r)) => (mb.to_string(), r.trim()),
            None => (before_literal.to_string(), ""),
        }
    };

    let flags = if rest.is_empty() {
        None
    } else {
        Some(rest.to_string())
    };

    Ok(ImapCommand::Append { mailbox, flags, literal_size })
}

/// remove surrounding quotes from a string
fn unquote(s: &str) -> String {
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_login() {
        let cmd = parse_command("a001 LOGIN user@example.com password123").unwrap();
        assert_eq!(cmd.tag, "a001");
        assert_eq!(
            cmd.command,
            ImapCommand::Login {
                username: "user@example.com".into(),
                password: "password123".into(),
            }
        );
    }

    #[test]
    fn parse_login_quoted() {
        let cmd = parse_command("a001 LOGIN \"user@example.com\" \"pass word\"").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Login {
                username: "user@example.com".into(),
                password: "pass word".into(),
            }
        );
    }

    #[test]
    fn parse_select() {
        let cmd = parse_command("a002 SELECT INBOX").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Select {
                mailbox: "INBOX".into()
            }
        );
    }

    #[test]
    fn parse_select_quoted() {
        let cmd = parse_command("a002 SELECT \"Sent Items\"").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Select {
                mailbox: "Sent Items".into()
            }
        );
    }

    #[test]
    fn parse_fetch() {
        let cmd = parse_command("a003 FETCH 1:* (FLAGS ENVELOPE)").unwrap();
        assert_eq!(cmd.tag, "a003");
        if let ImapCommand::Fetch {
            sequence,
            attributes,
        } = &cmd.command
        {
            assert_eq!(sequence, "1:*");
            assert_eq!(attributes, "(FLAGS ENVELOPE)");
        } else {
            panic!("expected Fetch");
        }
    }

    #[test]
    fn parse_store() {
        let cmd = parse_command("a004 STORE 1 +FLAGS (\\Seen)").unwrap();
        if let ImapCommand::Store {
            sequence,
            action,
            flags,
        } = &cmd.command
        {
            assert_eq!(sequence, "1");
            assert_eq!(action, "+FLAGS");
            assert_eq!(flags, "(\\Seen)");
        } else {
            panic!("expected Store");
        }
    }

    #[test]
    fn parse_search() {
        let cmd = parse_command("a005 SEARCH UNSEEN").unwrap();
        if let ImapCommand::Search { criteria } = &cmd.command {
            assert_eq!(criteria, "UNSEEN");
        } else {
            panic!("expected Search");
        }
    }

    #[test]
    fn parse_list() {
        let cmd = parse_command("a006 LIST \"\" \"*\"").unwrap();
        if let ImapCommand::List { reference, pattern } = &cmd.command {
            assert_eq!(reference, "");
            assert_eq!(pattern, "*");
        } else {
            panic!("expected List");
        }
    }

    #[test]
    fn parse_uid_fetch() {
        let cmd = parse_command("a007 UID FETCH 1:* FLAGS").unwrap();
        if let ImapCommand::Uid { subcommand } = &cmd.command {
            if let ImapCommand::Fetch { sequence, .. } = subcommand.as_ref() {
                assert_eq!(sequence, "1:*");
            } else {
                panic!("expected Fetch inside UID");
            }
        } else {
            panic!("expected Uid");
        }
    }

    #[test]
    fn parse_capability() {
        let cmd = parse_command("a008 CAPABILITY").unwrap();
        assert_eq!(cmd.command, ImapCommand::Capability);
    }

    #[test]
    fn parse_noop() {
        let cmd = parse_command("a009 NOOP").unwrap();
        assert_eq!(cmd.command, ImapCommand::Noop);
    }

    #[test]
    fn parse_case_insensitive() {
        let cmd = parse_command("a001 select INBOX").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Select {
                mailbox: "INBOX".into()
            }
        );
    }

    #[test]
    fn parse_append() {
        let cmd = parse_command("a001 APPEND INBOX {310}").unwrap();
        if let ImapCommand::Append { mailbox, flags, literal_size } = &cmd.command {
            assert_eq!(mailbox, "INBOX");
            assert!(flags.is_none());
            assert_eq!(*literal_size, 310);
        } else {
            panic!("expected Append");
        }
    }

    #[test]
    fn parse_append_with_flags() {
        let cmd = parse_command("a001 APPEND \"Drafts\" (\\Seen \\Draft) {100}").unwrap();
        if let ImapCommand::Append { mailbox, flags, literal_size } = &cmd.command {
            assert_eq!(mailbox, "Drafts");
            assert_eq!(flags.as_deref(), Some("(\\Seen \\Draft)"));
            assert_eq!(*literal_size, 100);
        } else {
            panic!("expected Append");
        }
    }

    #[test]
    fn parse_copy() {
        let cmd = parse_command("a001 COPY 1:* INBOX").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Copy {
                sequence: "1:*".into(),
                mailbox: "INBOX".into(),
            }
        );
    }

    #[test]
    fn parse_move() {
        let cmd = parse_command("a001 MOVE 1:3 Trash").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Move {
                sequence: "1:3".into(),
                mailbox: "Trash".into(),
            }
        );
    }

    #[test]
    fn parse_copy_quoted_mailbox() {
        let cmd = parse_command("a001 COPY 1 \"Sent Items\"").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Copy {
                sequence: "1".into(),
                mailbox: "Sent Items".into(),
            }
        );
    }

    #[test]
    fn parse_unknown_command() {
        let result = parse_command("a001 FOOBAR");
        assert!(matches!(result, Err(ParseError::UnknownCommand(_))));
    }
}
