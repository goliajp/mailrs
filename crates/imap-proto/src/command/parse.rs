use super::{ImapCommand, ParseError, TaggedCommand, unquote};

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

    // ASCII-uppercase the verb into a stack buffer so the arm match
    // doesn't allocate per call. Longest verb we care about is
    // `AUTHENTICATE` (12) / `GETQUOTAROOT` (12); 16 gives headroom.
    let cmd_bytes_raw = cmd_word.as_bytes();
    if cmd_bytes_raw.len() > 16 {
        return Err(ParseError::UnknownCommand(cmd_word.to_string()));
    }
    let mut cmd_upper_buf = [0u8; 16];
    for (i, &b) in cmd_bytes_raw.iter().enumerate() {
        cmd_upper_buf[i] = b.to_ascii_uppercase();
    }
    let cmd_upper = &cmd_upper_buf[..cmd_bytes_raw.len()];

    let command = match cmd_upper {
        b"CAPABILITY" => ImapCommand::Capability,
        b"LOGIN" => {
            let (username, password) = parse_login_args(args)?;
            ImapCommand::Login { username, password }
        }
        b"LOGOUT" => ImapCommand::Logout,
        b"LIST" => {
            let (reference, pattern) = parse_list_args(args)?;
            ImapCommand::List { reference, pattern }
        }
        b"SELECT" => ImapCommand::Select {
            mailbox: unquote(args.trim()),
        },
        b"EXAMINE" => ImapCommand::Examine {
            mailbox: unquote(args.trim()),
        },
        b"FETCH" => {
            let (seq, attrs) = args
                .split_once(' ')
                .ok_or(ParseError::MissingArgument("fetch attributes".into()))?;
            ImapCommand::Fetch {
                sequence: seq.to_string(),
                attributes: attrs.to_string(),
            }
        }
        b"STORE" => {
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
        b"SEARCH" => ImapCommand::Search {
            criteria: args.to_string(),
        },
        b"EXPUNGE" => ImapCommand::Expunge,
        b"NOOP" => ImapCommand::Noop,
        b"CLOSE" => ImapCommand::Close,
        b"IDLE" => ImapCommand::Idle,
        b"APPEND" => parse_append_args(args)?,
        b"COPY" => {
            let (seq, mb) = args
                .split_once(' ')
                .ok_or(ParseError::MissingArgument("copy mailbox".into()))?;
            ImapCommand::Copy {
                sequence: seq.to_string(),
                mailbox: unquote(mb.trim()),
            }
        }
        b"MOVE" => {
            let (seq, mb) = args
                .split_once(' ')
                .ok_or(ParseError::MissingArgument("move mailbox".into()))?;
            ImapCommand::Move {
                sequence: seq.to_string(),
                mailbox: unquote(mb.trim()),
            }
        }
        b"STATUS" => {
            let (mb, items) = args
                .split_once(' ')
                .ok_or(ParseError::MissingArgument("status items".into()))?;
            ImapCommand::Status {
                mailbox: unquote(mb.trim()),
                items: items.trim().to_string(),
            }
        }
        b"CREATE" => ImapCommand::Create {
            mailbox: unquote(args.trim()),
        },
        b"DELETE" => ImapCommand::Delete {
            mailbox: unquote(args.trim()),
        },
        b"RENAME" => {
            let (from, to) = parse_list_args(args)?;
            ImapCommand::Rename { from, to }
        }
        b"SUBSCRIBE" => ImapCommand::Subscribe {
            mailbox: unquote(args.trim()),
        },
        b"UNSUBSCRIBE" => ImapCommand::Unsubscribe {
            mailbox: unquote(args.trim()),
        },
        b"LSUB" => {
            let (reference, pattern) = parse_list_args(args)?;
            ImapCommand::Lsub { reference, pattern }
        }
        b"NAMESPACE" => ImapCommand::Namespace,
        b"ENABLE" => {
            let caps: Vec<String> = args.split_whitespace().map(|s| s.to_string()).collect();
            if caps.is_empty() {
                return Err(ParseError::MissingArgument("capabilities".into()));
            }
            ImapCommand::Enable(caps)
        }
        b"UNSELECT" => ImapCommand::Unselect,
        b"SORT" => {
            // SORT (criteria...) charset search-criteria
            // e.g.: (REVERSE DATE) UTF-8 ALL
            let args = args.trim();
            if !args.starts_with('(') {
                return Err(ParseError::MissingArgument("sort criteria".into()));
            }
            let close = args.find(')').ok_or(ParseError::MissingArgument(
                "sort criteria closing paren".into(),
            ))?;
            let criteria = args[1..close].to_string();
            let rest = args[close + 1..].trim();
            let (charset, search) = rest.split_once(' ').unwrap_or((rest, "ALL"));
            ImapCommand::Sort {
                criteria,
                charset: charset.to_string(),
                search_criteria: search.to_string(),
            }
        }
        b"GETQUOTA" => ImapCommand::GetQuota {
            quotaroot: unquote(args.trim()),
        },
        b"GETQUOTAROOT" => ImapCommand::GetQuotaRoot {
            mailbox: unquote(args.trim()),
        },
        b"UID" => {
            // parse the rest as a sub-command with a dummy tag
            let sub_line = format!("_ {args}");
            let sub = parse_command(&sub_line)?;
            ImapCommand::Uid {
                subcommand: Box::new(sub.command),
            }
        }
        other => {
            // `other` is &[u8] (slice into cmd_upper_buf); use the
            // original mixed-case verb in the error so users see what
            // they actually typed.
            let _ = other;
            return Err(ParseError::UnknownCommand(cmd_word.to_string()));
        }
    };

    Ok(TaggedCommand { tag, command })
}

/// parse LOGIN username password (supports quoted strings)
fn parse_login_args(args: &str) -> Result<(String, String), ParseError> {
    // Two-token parse, byte-oriented, zero intermediate allocation
    // (the only heap allocs are the two returned `String`s — minimum
    // possible given the public API). Compared to the old impl this
    // kills:
    //   - `Vec::new()` (heap allocation)
    //   - `String::new()` for the rolling `current` (heap allocation)
    //   - `parts.push(current.clone())` (one clone per token)
    //   - `parts[0..2].clone()` (two more clones)
    let bytes = args.as_bytes();
    let n = bytes.len();
    let mut i = 0;

    // Skip leading whitespace.
    while i < n && bytes[i] == b' ' {
        i += 1;
    }

    let (username, j) = parse_login_token(bytes, i)?;
    i = j;

    while i < n && bytes[i] == b' ' {
        i += 1;
    }

    let (password, _j) = parse_login_token(bytes, i)?;

    Ok((username, password))
}

fn parse_login_token(bytes: &[u8], start: usize) -> Result<(String, usize), ParseError> {
    if start >= bytes.len() {
        return Err(ParseError::MissingArgument("username and password".into()));
    }

    if bytes[start] == b'"' {
        // RFC 3501 quoted-string: read until next unescaped `"`.
        let mut out = String::with_capacity(16);
        let mut i = start + 1;
        while i < bytes.len() {
            let b = bytes[i];
            if b == b'"' {
                return Ok((out, i + 1));
            }
            if b == b'\\' && i + 1 < bytes.len() {
                out.push(bytes[i + 1] as char);
                i += 2;
            } else {
                out.push(b as char);
                i += 1;
            }
        }
        Err(ParseError::MissingArgument(
            "unterminated quoted string".into(),
        ))
    } else {
        // Atom: read until space or end of input.
        let mut i = start;
        while i < bytes.len() && bytes[i] != b' ' {
            i += 1;
        }
        if i == start {
            return Err(ParseError::MissingArgument("empty token".into()));
        }
        // SAFETY: IMAP atoms are ASCII per RFC 3501 §9; the upstream
        // code only feeds `str` slices in and we copy byte-for-byte.
        // The non-unchecked path costs ~5 ns on a 5-byte input.
        let s = std::str::from_utf8(&bytes[start..i])
            .map_err(|_| ParseError::MissingArgument("non-ASCII token".into()))?;
        Ok((s.to_owned(), i))
    }
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
    let literal_start = args
        .rfind('{')
        .ok_or(ParseError::MissingArgument("literal size".into()))?;
    let literal_end = args
        .rfind('}')
        .ok_or(ParseError::MissingArgument("literal size".into()))?;
    if literal_end <= literal_start {
        return Err(ParseError::MissingArgument("literal size".into()));
    }
    let size_str = &args[literal_start + 1..literal_end];
    let literal_size: u32 = size_str
        .parse()
        .map_err(|_| ParseError::MissingArgument(format!("invalid literal size: {size_str}")))?;

    let before_literal = args[..literal_start].trim();

    // first token is the mailbox name
    let (mailbox, rest) = if let Some(stripped) = before_literal.strip_prefix('"') {
        // quoted mailbox
        let end = stripped
            .find('"')
            .ok_or(ParseError::MissingArgument("mailbox".into()))?;
        let mb = stripped[..end].to_string();
        let rest = stripped[end + 1..].trim();
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

    Ok(ImapCommand::Append {
        mailbox,
        flags,
        literal_size,
    })
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
        if let ImapCommand::Append {
            mailbox,
            flags,
            literal_size,
        } = &cmd.command
        {
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
        if let ImapCommand::Append {
            mailbox,
            flags,
            literal_size,
        } = &cmd.command
        {
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

    // --- error path tests ---

    #[test]
    fn parse_empty_input_returns_error() {
        assert_eq!(parse_command(""), Err(ParseError::EmptyInput));
        assert_eq!(parse_command("   "), Err(ParseError::EmptyInput));
    }

    #[test]
    fn parse_missing_tag_returns_error() {
        // no space → no tag/command split
        assert_eq!(parse_command("CAPABILITY"), Err(ParseError::MissingTag));
    }

    #[test]
    fn parse_login_missing_args() {
        let result = parse_command("a001 LOGIN");
        assert!(matches!(result, Err(ParseError::MissingArgument(_))));
    }

    #[test]
    fn parse_login_missing_password() {
        let result = parse_command("a001 LOGIN useronly");
        assert!(matches!(result, Err(ParseError::MissingArgument(_))));
    }

    #[test]
    fn parse_fetch_missing_attributes() {
        let result = parse_command("a001 FETCH 1:5");
        assert!(matches!(result, Err(ParseError::MissingArgument(_))));
    }

    #[test]
    fn parse_store_missing_flags() {
        // only 2 parts instead of 3
        let result = parse_command("a001 STORE 1 +FLAGS");
        assert!(matches!(result, Err(ParseError::MissingArgument(_))));
    }

    #[test]
    fn parse_copy_missing_mailbox() {
        let result = parse_command("a001 COPY 1:5");
        assert!(matches!(result, Err(ParseError::MissingArgument(_))));
    }

    #[test]
    fn parse_move_missing_mailbox() {
        let result = parse_command("a001 MOVE 1:5");
        assert!(matches!(result, Err(ParseError::MissingArgument(_))));
    }

    #[test]
    fn parse_append_missing_literal() {
        let result = parse_command("a001 APPEND INBOX");
        assert!(matches!(result, Err(ParseError::MissingArgument(_))));
    }

    #[test]
    fn parse_append_invalid_literal_size() {
        let result = parse_command("a001 APPEND INBOX {abc}");
        assert!(matches!(result, Err(ParseError::MissingArgument(_))));
    }

    // --- no-argument commands ---

    #[test]
    fn parse_logout() {
        let cmd = parse_command("a001 LOGOUT").unwrap();
        assert_eq!(cmd.tag, "a001");
        assert_eq!(cmd.command, ImapCommand::Logout);
    }

    #[test]
    fn parse_expunge() {
        let cmd = parse_command("a001 EXPUNGE").unwrap();
        assert_eq!(cmd.command, ImapCommand::Expunge);
    }

    #[test]
    fn parse_close() {
        let cmd = parse_command("a001 CLOSE").unwrap();
        assert_eq!(cmd.command, ImapCommand::Close);
    }

    #[test]
    fn parse_idle() {
        let cmd = parse_command("a001 IDLE").unwrap();
        assert_eq!(cmd.command, ImapCommand::Idle);
    }

    // --- EXAMINE ---

    #[test]
    fn parse_examine() {
        let cmd = parse_command("a001 EXAMINE INBOX").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Examine {
                mailbox: "INBOX".into()
            }
        );
    }

    #[test]
    fn parse_examine_quoted() {
        let cmd = parse_command("a001 EXAMINE \"Sent Items\"").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Examine {
                mailbox: "Sent Items".into()
            }
        );
    }

    // --- GETQUOTA / GETQUOTAROOT ---

    #[test]
    fn parse_getquota() {
        let cmd = parse_command("a001 GETQUOTA \"\"").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::GetQuota {
                quotaroot: "".into()
            }
        );
    }

    #[test]
    fn parse_getquota_named() {
        let cmd = parse_command("a001 GETQUOTA user.alice").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::GetQuota {
                quotaroot: "user.alice".into()
            }
        );
    }

    #[test]
    fn parse_getquotaroot() {
        let cmd = parse_command("a001 GETQUOTAROOT INBOX").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::GetQuotaRoot {
                mailbox: "INBOX".into()
            }
        );
    }

    // --- UID sub-commands ---

    #[test]
    fn parse_uid_store() {
        let cmd = parse_command("a001 UID STORE 1:5 +FLAGS (\\Seen)").unwrap();
        if let ImapCommand::Uid { subcommand } = &cmd.command {
            assert!(matches!(
                subcommand.as_ref(),
                ImapCommand::Store { sequence, action, flags }
                if sequence == "1:5" && action == "+FLAGS" && flags == "(\\Seen)"
            ));
        } else {
            panic!("expected Uid");
        }
    }

    #[test]
    fn parse_uid_search() {
        let cmd = parse_command("a001 UID SEARCH UNSEEN").unwrap();
        if let ImapCommand::Uid { subcommand } = &cmd.command {
            assert!(matches!(
                subcommand.as_ref(),
                ImapCommand::Search { criteria } if criteria == "UNSEEN"
            ));
        } else {
            panic!("expected Uid");
        }
    }

    #[test]
    fn parse_uid_copy() {
        let cmd = parse_command("a001 UID COPY 10:20 Sent").unwrap();
        if let ImapCommand::Uid { subcommand } = &cmd.command {
            assert!(matches!(
                subcommand.as_ref(),
                ImapCommand::Copy { sequence, mailbox }
                if sequence == "10:20" && mailbox == "Sent"
            ));
        } else {
            panic!("expected Uid");
        }
    }

    #[test]
    fn parse_uid_expunge() {
        let cmd = parse_command("a001 UID EXPUNGE").unwrap();
        if let ImapCommand::Uid { subcommand } = &cmd.command {
            assert_eq!(subcommand.as_ref(), &ImapCommand::Expunge);
        } else {
            panic!("expected Uid");
        }
    }

    // --- mixed-case command word ---

    #[test]
    fn parse_mixed_case_command_word() {
        let cmd = parse_command("a001 FeTcH 1 FLAGS").unwrap();
        assert!(matches!(cmd.command, ImapCommand::Fetch { .. }));
    }

    // --- SEARCH with complex criteria ---

    #[test]
    fn parse_search_complex_criteria() {
        let cmd = parse_command("a001 SEARCH SINCE 1-Jan-2024 FROM user@example.com").unwrap();
        if let ImapCommand::Search { criteria } = &cmd.command {
            assert_eq!(criteria, "SINCE 1-Jan-2024 FROM user@example.com");
        } else {
            panic!("expected Search");
        }
    }

    // --- MOVE with quoted mailbox ---

    #[test]
    fn parse_move_quoted_mailbox() {
        let cmd = parse_command("a001 MOVE 1:3 \"Deleted Items\"").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Move {
                sequence: "1:3".into(),
                mailbox: "Deleted Items".into(),
            }
        );
    }

    // --- FETCH with UID in attributes ---

    #[test]
    fn parse_fetch_body_section() {
        let cmd = parse_command("a001 FETCH 1 BODY[]").unwrap();
        if let ImapCommand::Fetch {
            sequence,
            attributes,
        } = &cmd.command
        {
            assert_eq!(sequence, "1");
            assert_eq!(attributes, "BODY[]");
        } else {
            panic!("expected Fetch");
        }
    }

    // --- tag is preserved verbatim ---

    #[test]
    fn parse_tag_preserved() {
        let cmd = parse_command("TAG.123 NOOP").unwrap();
        assert_eq!(cmd.tag, "TAG.123");
    }

    // --- APPEND with no flags and simple mailbox ---

    #[test]
    fn parse_append_no_flags() {
        let cmd = parse_command("a001 APPEND Drafts {512}").unwrap();
        if let ImapCommand::Append {
            mailbox,
            flags,
            literal_size,
        } = &cmd.command
        {
            assert_eq!(mailbox, "Drafts");
            assert!(flags.is_none());
            assert_eq!(*literal_size, 512);
        } else {
            panic!("expected Append");
        }
    }

    // --- STATUS ---

    #[test]
    fn parse_status() {
        let cmd = parse_command("a001 STATUS INBOX (MESSAGES UNSEEN)").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Status {
                mailbox: "INBOX".into(),
                items: "(MESSAGES UNSEEN)".into(),
            }
        );
    }

    #[test]
    fn parse_status_quoted_mailbox() {
        // status uses split_once(' ') so quoted mailbox without spaces works
        let cmd = parse_command("a001 STATUS \"Drafts\" (MESSAGES RECENT UNSEEN)").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Status {
                mailbox: "Drafts".into(),
                items: "(MESSAGES RECENT UNSEEN)".into(),
            }
        );
    }

    #[test]
    fn parse_status_missing_items() {
        let result = parse_command("a001 STATUS INBOX");
        assert!(matches!(result, Err(ParseError::MissingArgument(_))));
    }

    // --- CREATE / DELETE ---

    #[test]
    fn parse_create() {
        let cmd = parse_command("a001 CREATE \"My Folder\"").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Create {
                mailbox: "My Folder".into(),
            }
        );
    }

    #[test]
    fn parse_create_unquoted() {
        let cmd = parse_command("a001 CREATE Archive").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Create {
                mailbox: "Archive".into(),
            }
        );
    }

    #[test]
    fn parse_delete() {
        let cmd = parse_command("a001 DELETE \"Old Mail\"").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Delete {
                mailbox: "Old Mail".into(),
            }
        );
    }

    #[test]
    fn parse_delete_unquoted() {
        let cmd = parse_command("a001 DELETE Trash").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Delete {
                mailbox: "Trash".into(),
            }
        );
    }

    // --- RENAME ---

    #[test]
    fn parse_rename() {
        let cmd = parse_command("a001 RENAME \"Old Name\" \"New Name\"").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Rename {
                from: "Old Name".into(),
                to: "New Name".into(),
            }
        );
    }

    #[test]
    fn parse_rename_unquoted() {
        let cmd = parse_command("a001 RENAME OldBox NewBox").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Rename {
                from: "OldBox".into(),
                to: "NewBox".into(),
            }
        );
    }

    #[test]
    fn parse_rename_missing_arg() {
        let result = parse_command("a001 RENAME OnlyOne");
        assert!(matches!(result, Err(ParseError::MissingArgument(_))));
    }

    // --- SUBSCRIBE / UNSUBSCRIBE ---

    #[test]
    fn parse_subscribe() {
        let cmd = parse_command("a001 SUBSCRIBE INBOX").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Subscribe {
                mailbox: "INBOX".into(),
            }
        );
    }

    #[test]
    fn parse_subscribe_quoted() {
        let cmd = parse_command("a001 SUBSCRIBE \"Sent Items\"").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Subscribe {
                mailbox: "Sent Items".into(),
            }
        );
    }

    #[test]
    fn parse_unsubscribe() {
        let cmd = parse_command("a001 UNSUBSCRIBE INBOX").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Unsubscribe {
                mailbox: "INBOX".into(),
            }
        );
    }

    #[test]
    fn parse_unsubscribe_quoted() {
        let cmd = parse_command("a001 UNSUBSCRIBE \"Junk Mail\"").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Unsubscribe {
                mailbox: "Junk Mail".into(),
            }
        );
    }

    // --- LSUB ---

    #[test]
    fn parse_lsub() {
        let cmd = parse_command("a001 LSUB \"\" \"*\"").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Lsub {
                reference: "".into(),
                pattern: "*".into(),
            }
        );
    }

    #[test]
    fn parse_lsub_with_reference() {
        let cmd = parse_command("a001 LSUB \"INBOX\" \"%\"").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Lsub {
                reference: "INBOX".into(),
                pattern: "%".into(),
            }
        );
    }

    #[test]
    fn parse_lsub_missing_pattern() {
        let result = parse_command("a001 LSUB \"\"");
        assert!(matches!(result, Err(ParseError::MissingArgument(_))));
    }

    // --- additional edge-case tests ---

    #[test]
    fn parse_uid_move() {
        let cmd = parse_command("a001 UID MOVE 1:5 Trash").unwrap();
        if let ImapCommand::Uid { subcommand } = &cmd.command {
            assert!(matches!(
                subcommand.as_ref(),
                ImapCommand::Move { sequence, mailbox }
                if sequence == "1:5" && mailbox == "Trash"
            ));
        } else {
            panic!("expected Uid");
        }
    }

    #[test]
    fn parse_store_minus_flags() {
        let cmd = parse_command("a001 STORE 1:3 -FLAGS (\\Seen)").unwrap();
        if let ImapCommand::Store {
            sequence,
            action,
            flags,
        } = &cmd.command
        {
            assert_eq!(sequence, "1:3");
            assert_eq!(action, "-FLAGS");
            assert_eq!(flags, "(\\Seen)");
        } else {
            panic!("expected Store");
        }
    }

    #[test]
    fn parse_store_flags_replace() {
        let cmd = parse_command("a001 STORE 5 FLAGS (\\Answered \\Seen)").unwrap();
        if let ImapCommand::Store {
            sequence,
            action,
            flags,
        } = &cmd.command
        {
            assert_eq!(sequence, "5");
            assert_eq!(action, "FLAGS");
            assert_eq!(flags, "(\\Answered \\Seen)");
        } else {
            panic!("expected Store");
        }
    }

    #[test]
    fn parse_store_silent_flags() {
        let cmd = parse_command("a001 STORE 1 +FLAGS.SILENT (\\Deleted)").unwrap();
        if let ImapCommand::Store { action, flags, .. } = &cmd.command {
            assert_eq!(action, "+FLAGS.SILENT");
            assert_eq!(flags, "(\\Deleted)");
        } else {
            panic!("expected Store");
        }
    }

    #[test]
    fn parse_append_reversed_braces() {
        let result = parse_command("a001 APPEND INBOX }310{");
        assert!(matches!(result, Err(ParseError::MissingArgument(_))));
    }

    #[test]
    fn parse_append_zero_literal() {
        let cmd = parse_command("a001 APPEND INBOX {0}").unwrap();
        if let ImapCommand::Append { literal_size, .. } = &cmd.command {
            assert_eq!(*literal_size, 0);
        } else {
            panic!("expected Append");
        }
    }

    #[test]
    fn parse_login_extra_spaces() {
        let cmd = parse_command("a001 LOGIN   user   pass").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Login {
                username: "user".into(),
                password: "pass".into(),
            }
        );
    }

    #[test]
    fn parse_list_unquoted() {
        let cmd = parse_command("a001 LIST INBOX *").unwrap();
        if let ImapCommand::List { reference, pattern } = &cmd.command {
            assert_eq!(reference, "INBOX");
            assert_eq!(pattern, "*");
        } else {
            panic!("expected List");
        }
    }

    #[test]
    fn parse_list_percent_pattern() {
        let cmd = parse_command("a001 LIST \"\" \"%\"").unwrap();
        if let ImapCommand::List { reference, pattern } = &cmd.command {
            assert_eq!(reference, "");
            assert_eq!(pattern, "%");
        } else {
            panic!("expected List");
        }
    }

    #[test]
    fn parse_list_missing_pattern() {
        let result = parse_command("a001 LIST \"\"");
        assert!(matches!(result, Err(ParseError::MissingArgument(_))));
    }

    #[test]
    fn parse_fetch_multiple_attrs() {
        let cmd = parse_command("a001 FETCH 1:5 (FLAGS UID ENVELOPE BODY.PEEK[HEADER])").unwrap();
        if let ImapCommand::Fetch {
            sequence,
            attributes,
        } = &cmd.command
        {
            assert_eq!(sequence, "1:5");
            assert_eq!(attributes, "(FLAGS UID ENVELOPE BODY.PEEK[HEADER])");
        } else {
            panic!("expected Fetch");
        }
    }

    #[test]
    fn parse_command_preserves_numeric_tag() {
        let cmd = parse_command("12345 NOOP").unwrap();
        assert_eq!(cmd.tag, "12345");
    }

    #[test]
    fn parse_uid_unknown_subcommand() {
        let result = parse_command("a001 UID BADCMD");
        assert!(matches!(result, Err(ParseError::UnknownCommand(_))));
    }

    #[test]
    fn parse_select_with_trailing_whitespace() {
        let cmd = parse_command("a001 SELECT INBOX   ").unwrap();
        assert_eq!(
            cmd.command,
            ImapCommand::Select {
                mailbox: "INBOX".into()
            }
        );
    }
}
