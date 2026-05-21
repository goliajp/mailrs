/// A parsed IMAP4rev1 command. Each variant owns its argument strings
/// (commands cross network boundaries, so we don't borrow).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImapCommand {
    /// `CAPABILITY` — list supported extensions.
    Capability,
    /// `LOGIN <user> <pass>` — authenticate with plain credentials.
    Login {
        /// Username argument.
        username: String,
        /// Password argument (plaintext per RFC 3501; clients should use STARTTLS).
        password: String,
    },
    /// `LOGOUT` — close the connection.
    Logout,
    /// `LIST <reference> <pattern>` — enumerate matching mailbox names.
    List {
        /// Reference name (usually empty string).
        reference: String,
        /// Pattern with `%` (single-level) and `*` (recursive) wildcards.
        pattern: String,
    },
    /// `SELECT <mailbox>` — open a mailbox for read/write.
    Select {
        /// Mailbox name.
        mailbox: String,
    },
    /// `EXAMINE <mailbox>` — open a mailbox read-only.
    Examine {
        /// Mailbox name.
        mailbox: String,
    },
    /// `FETCH <seq> <attrs>` — retrieve message attributes / parts.
    Fetch {
        /// Sequence set (`1:10`, `*`, `1,3,5`, etc).
        sequence: String,
        /// Attribute spec (`FLAGS`, `BODY[]`, `(FLAGS BODY.PEEK[HEADER])`, ...).
        attributes: String,
    },
    /// `STORE <seq> <action> <flags>` — set/add/remove flags.
    Store {
        /// Sequence set.
        sequence: String,
        /// One of `FLAGS` / `+FLAGS` / `-FLAGS` (+`.SILENT` variant).
        action: String,
        /// Flag list (e.g. `(\Seen)`).
        flags: String,
    },
    /// `SEARCH <criteria>` — return UIDs matching the search keys.
    Search {
        /// Search criteria string (`UNSEEN FROM alice@x`, etc).
        criteria: String,
    },
    /// `EXPUNGE` — purge messages with the `\Deleted` flag.
    Expunge,
    /// `NOOP` — no-op (kept for keepalive + STATUS-update side effect).
    Noop,
    /// `CLOSE` — close the current mailbox (expunge implicit).
    Close,
    /// `IDLE` — push notifications until `DONE` (RFC 2177).
    Idle,
    /// `APPEND <mailbox> [flags] {n}<CRLF>...` — upload a new message.
    Append {
        /// Target mailbox.
        mailbox: String,
        /// Optional initial flag list (e.g. `(\Seen \Flagged)`).
        flags: Option<String>,
        /// Literal byte count for the message body that follows.
        literal_size: u32,
    },
    /// `COPY <seq> <mailbox>` — copy messages to another mailbox.
    Copy {
        /// Sequence set in the source mailbox.
        sequence: String,
        /// Destination mailbox name.
        mailbox: String,
    },
    /// `MOVE <seq> <mailbox>` (RFC 6851) — move messages.
    Move {
        /// Sequence set in the source mailbox.
        sequence: String,
        /// Destination mailbox name.
        mailbox: String,
    },
    /// `UID <subcommand>` — re-interpret the subcommand's sequence set as UIDs.
    Uid {
        /// The nested IMAP command operating on UIDs.
        subcommand: Box<ImapCommand>,
    },
    /// `STATUS <mailbox> <items>` — read mailbox-level counts.
    Status {
        /// Mailbox name to inspect.
        mailbox: String,
        /// Item list (e.g. `(MESSAGES UNSEEN UIDNEXT UIDVALIDITY HIGHESTMODSEQ)`).
        items: String,
    },
    /// `GETQUOTA <quotaroot>` — read a quota resource (RFC 2087).
    GetQuota {
        /// Quota root identifier.
        quotaroot: String,
    },
    /// `GETQUOTAROOT <mailbox>` — list quota roots applying to a mailbox.
    GetQuotaRoot {
        /// Mailbox name.
        mailbox: String,
    },
    /// `CREATE <mailbox>` — create a mailbox.
    Create {
        /// New mailbox name.
        mailbox: String,
    },
    /// `DELETE <mailbox>` — delete a mailbox.
    Delete {
        /// Mailbox to delete.
        mailbox: String,
    },
    /// `RENAME <from> <to>` — rename a mailbox.
    Rename {
        /// Existing mailbox name.
        from: String,
        /// New mailbox name.
        to: String,
    },
    /// `SUBSCRIBE <mailbox>` — add a mailbox to the user's active list.
    Subscribe {
        /// Mailbox to subscribe to.
        mailbox: String,
    },
    /// `UNSUBSCRIBE <mailbox>` — remove from subscription list.
    Unsubscribe {
        /// Mailbox to unsubscribe.
        mailbox: String,
    },
    /// `LSUB <reference> <pattern>` — list subscribed mailboxes.
    Lsub {
        /// Reference name (usually empty).
        reference: String,
        /// Pattern.
        pattern: String,
    },
    /// `NAMESPACE` — return per-user / per-shared / per-other namespace prefixes.
    Namespace,
    /// `SORT <criteria> <charset> <search>` — server-side sorted SEARCH
    /// (RFC 5256).
    Sort {
        /// Sort criteria (e.g. `(DATE)` / `(REVERSE ARRIVAL)`).
        criteria: String,
        /// Charset for text values in search criteria.
        charset: String,
        /// Search criteria (filter applied before sort).
        search_criteria: String,
    },
    /// `ENABLE <ext>+` (RFC 5161) — opt into named extensions.
    Enable(Vec<String>),
    /// `UNSELECT` (RFC 3691) — close mailbox without expunging.
    Unselect,
}

/// An IMAP command paired with its tag (the client-chosen identifier
/// echoed back in the matching tagged response, e.g. `"a001"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedCommand {
    /// Client-chosen tag (e.g. `"a001"`).
    pub tag: String,
    /// Parsed command.
    pub command: ImapCommand,
}

/// Error returned by [`parse_command`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Input line was empty after trimming.
    EmptyInput,
    /// No space-separated tag prefix was found.
    MissingTag,
    /// Command verb did not match any known IMAP command.
    UnknownCommand(String),
    /// Command was recognized but a required argument was missing.
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
        "STATUS" => {
            let (mb, items) = args
                .split_once(' ')
                .ok_or(ParseError::MissingArgument("status items".into()))?;
            ImapCommand::Status {
                mailbox: unquote(mb.trim()),
                items: items.trim().to_string(),
            }
        }
        "CREATE" => ImapCommand::Create {
            mailbox: unquote(args.trim()),
        },
        "DELETE" => ImapCommand::Delete {
            mailbox: unquote(args.trim()),
        },
        "RENAME" => {
            let (from, to) = parse_list_args(args)?;
            ImapCommand::Rename { from, to }
        }
        "SUBSCRIBE" => ImapCommand::Subscribe {
            mailbox: unquote(args.trim()),
        },
        "UNSUBSCRIBE" => ImapCommand::Unsubscribe {
            mailbox: unquote(args.trim()),
        },
        "LSUB" => {
            let (reference, pattern) = parse_list_args(args)?;
            ImapCommand::Lsub { reference, pattern }
        }
        "NAMESPACE" => ImapCommand::Namespace,
        "ENABLE" => {
            let caps: Vec<String> = args.split_whitespace().map(|s| s.to_string()).collect();
            if caps.is_empty() {
                return Err(ParseError::MissingArgument("capabilities".into()));
            }
            ImapCommand::Enable(caps)
        }
        "UNSELECT" => ImapCommand::Unselect,
        "SORT" => {
            // SORT (criteria...) charset search-criteria
            // e.g.: (REVERSE DATE) UTF-8 ALL
            let args = args.trim();
            if !args.starts_with('(') {
                return Err(ParseError::MissingArgument("sort criteria".into()));
            }
            let close = args.find(')').ok_or(ParseError::MissingArgument("sort criteria closing paren".into()))?;
            let criteria = args[1..close].to_string();
            let rest = args[close + 1..].trim();
            let (charset, search) = rest.split_once(' ').unwrap_or((rest, "ALL"));
            ImapCommand::Sort {
                criteria,
                charset: charset.to_string(),
                search_criteria: search.to_string(),
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
    let (mailbox, rest) = if let Some(stripped) = before_literal.strip_prefix('"') {
        // quoted mailbox
        let end = stripped.find('"')
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

    Ok(ImapCommand::Append { mailbox, flags, literal_size })
}

/// Parsed IMAP SEARCH key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchKey {
    /// `ALL` — match every message in the mailbox.
    All,
    /// `SEEN` — messages with the `\Seen` flag.
    Seen,
    /// `UNSEEN` — messages without `\Seen`.
    Unseen,
    /// `FLAGGED` — messages with `\Flagged`.
    Flagged,
    /// `UNFLAGGED` — messages without `\Flagged`.
    Unflagged,
    /// `ANSWERED` — messages with `\Answered`.
    Answered,
    /// `UNANSWERED` — messages without `\Answered`.
    Unanswered,
    /// `DELETED` — messages with `\Deleted`.
    Deleted,
    /// `UNDELETED` — messages without `\Deleted`.
    Undeleted,
    /// `DRAFT` — messages with `\Draft`.
    Draft,
    /// `UNDRAFT` — messages without `\Draft`.
    Undraft,
    /// `RECENT` — messages with `\Recent` (per-session).
    Recent,
    /// `FROM <string>` — substring match on the From header.
    From(String),
    /// `TO <string>` — substring match on the To header.
    To(String),
    /// `SUBJECT <string>` — substring match on the Subject header.
    Subject(String),
    /// `TEXT <string>` — substring match on headers + body.
    Text(String),
    /// `BODY <string>` — substring match on body only.
    Body(String),
    /// `SINCE <date>` — internal date on or after (epoch seconds).
    Since(i64),
    /// `BEFORE <date>` — internal date strictly before (epoch seconds).
    Before(i64),
    /// `ON <date>` — internal date matches that day (epoch seconds, start of day).
    On(i64),
    /// `UID <sequence-set>` — match specific UIDs.
    Uid(String),
}

/// parse IMAP SEARCH criteria string into a list of search keys
///
/// supports AND-combination (multiple keys all must match).
/// unknown tokens are silently skipped to stay compatible.
pub fn parse_search_criteria(criteria: &str) -> Vec<SearchKey> {
    let criteria = criteria.trim();
    if criteria.is_empty() {
        return vec![SearchKey::All];
    }

    let mut keys = Vec::new();
    let tokens = tokenize_search(criteria);
    let mut i = 0;

    while i < tokens.len() {
        let token_upper = tokens[i].to_uppercase();
        match token_upper.as_str() {
            "ALL" => keys.push(SearchKey::All),
            "SEEN" => keys.push(SearchKey::Seen),
            "UNSEEN" => keys.push(SearchKey::Unseen),
            "FLAGGED" => keys.push(SearchKey::Flagged),
            "UNFLAGGED" => keys.push(SearchKey::Unflagged),
            "ANSWERED" => keys.push(SearchKey::Answered),
            "UNANSWERED" => keys.push(SearchKey::Unanswered),
            "DELETED" => keys.push(SearchKey::Deleted),
            "UNDELETED" => keys.push(SearchKey::Undeleted),
            "DRAFT" => keys.push(SearchKey::Draft),
            "UNDRAFT" => keys.push(SearchKey::Undraft),
            "RECENT" => keys.push(SearchKey::Recent),
            "FROM"
                if i + 1 < tokens.len() => {
                    i += 1;
                    keys.push(SearchKey::From(unquote(&tokens[i])));
                }
            "TO"
                if i + 1 < tokens.len() => {
                    i += 1;
                    keys.push(SearchKey::To(unquote(&tokens[i])));
                }
            "SUBJECT"
                if i + 1 < tokens.len() => {
                    i += 1;
                    keys.push(SearchKey::Subject(unquote(&tokens[i])));
                }
            "TEXT"
                if i + 1 < tokens.len() => {
                    i += 1;
                    keys.push(SearchKey::Text(unquote(&tokens[i])));
                }
            "BODY"
                if i + 1 < tokens.len() => {
                    i += 1;
                    keys.push(SearchKey::Body(unquote(&tokens[i])));
                }
            "SINCE"
                if i + 1 < tokens.len() => {
                    i += 1;
                    if let Some(ts) = parse_imap_date(&tokens[i]) {
                        keys.push(SearchKey::Since(ts));
                    }
                }
            "BEFORE"
                if i + 1 < tokens.len() => {
                    i += 1;
                    if let Some(ts) = parse_imap_date(&tokens[i]) {
                        keys.push(SearchKey::Before(ts));
                    }
                }
            "ON"
                if i + 1 < tokens.len() => {
                    i += 1;
                    if let Some(ts) = parse_imap_date(&tokens[i]) {
                        keys.push(SearchKey::On(ts));
                    }
                }
            "UID"
                if i + 1 < tokens.len() => {
                    i += 1;
                    keys.push(SearchKey::Uid(tokens[i].clone()));
                }
            // skip unknown tokens (e.g. "CHARSET", "UTF-8")
            _ => {}
        }
        i += 1;
    }

    if keys.is_empty() {
        keys.push(SearchKey::All);
    }
    keys
}

/// tokenize search criteria, respecting quoted strings
fn tokenize_search(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;

    for ch in input.chars() {
        match ch {
            '"' => {
                if in_quote {
                    // end of quoted string, push with quotes for unquote()
                    tokens.push(format!("\"{current}\""));
                    current.clear();
                    in_quote = false;
                } else {
                    if !current.is_empty() {
                        tokens.push(current.clone());
                        current.clear();
                    }
                    in_quote = true;
                }
            }
            ' ' if !in_quote => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// parse IMAP date format: d-Mon-yyyy (e.g. "1-Jan-2024" or "01-Jan-2024")
/// returns epoch seconds (start of day UTC)
fn parse_imap_date(s: &str) -> Option<i64> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let day: u32 = parts[0].parse().ok()?;
    let month = match parts[1].to_uppercase().as_str() {
        "JAN" => 1,
        "FEB" => 2,
        "MAR" => 3,
        "APR" => 4,
        "MAY" => 5,
        "JUN" => 6,
        "JUL" => 7,
        "AUG" => 8,
        "SEP" => 9,
        "OCT" => 10,
        "NOV" => 11,
        "DEC" => 12,
        _ => return None,
    };
    let year: i64 = parts[2].parse().ok()?;

    // simple date-to-epoch conversion (UTC, no leap second handling)
    let mut days: i64 = 0;
    // years since epoch
    for y in 1970..year {
        days += if is_leap_year(y) { 366 } else { 365 };
    }
    // months in current year
    let days_in_months = [31, 28 + if is_leap_year(year) { 1 } else { 0 },
        31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    for d in days_in_months.iter().take((month - 1) as usize) {
        days += *d as i64;
    }
    days += day as i64 - 1;
    Some(days * 86400)
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
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

    // --- ParseError Display ---

    #[test]
    fn parse_error_display() {
        assert_eq!(ParseError::EmptyInput.to_string(), "empty input");
        assert_eq!(ParseError::MissingTag.to_string(), "missing tag");
        assert_eq!(
            ParseError::UnknownCommand("FOO".into()).to_string(),
            "unknown command: FOO"
        );
        assert_eq!(
            ParseError::MissingArgument("username and password".into()).to_string(),
            "missing argument: username and password"
        );
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
        if let ImapCommand::Fetch { sequence, attributes } = &cmd.command {
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
        if let ImapCommand::Append { mailbox, flags, literal_size } = &cmd.command {
            assert_eq!(mailbox, "Drafts");
            assert!(flags.is_none());
            assert_eq!(*literal_size, 512);
        } else {
            panic!("expected Append");
        }
    }

    // --- SearchCriteria parsing tests ---

    #[test]
    fn search_criteria_empty_returns_all() {
        let keys = parse_search_criteria("");
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn search_criteria_all() {
        let keys = parse_search_criteria("ALL");
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn search_criteria_flag_keys() {
        assert_eq!(parse_search_criteria("SEEN"), vec![SearchKey::Seen]);
        assert_eq!(parse_search_criteria("UNSEEN"), vec![SearchKey::Unseen]);
        assert_eq!(parse_search_criteria("FLAGGED"), vec![SearchKey::Flagged]);
        assert_eq!(
            parse_search_criteria("UNFLAGGED"),
            vec![SearchKey::Unflagged]
        );
        assert_eq!(parse_search_criteria("ANSWERED"), vec![SearchKey::Answered]);
        assert_eq!(
            parse_search_criteria("UNANSWERED"),
            vec![SearchKey::Unanswered]
        );
        assert_eq!(parse_search_criteria("DELETED"), vec![SearchKey::Deleted]);
        assert_eq!(
            parse_search_criteria("UNDELETED"),
            vec![SearchKey::Undeleted]
        );
        assert_eq!(parse_search_criteria("DRAFT"), vec![SearchKey::Draft]);
        assert_eq!(parse_search_criteria("UNDRAFT"), vec![SearchKey::Undraft]);
        assert_eq!(parse_search_criteria("RECENT"), vec![SearchKey::Recent]);
    }

    #[test]
    fn search_criteria_case_insensitive() {
        assert_eq!(parse_search_criteria("unseen"), vec![SearchKey::Unseen]);
        assert_eq!(parse_search_criteria("Flagged"), vec![SearchKey::Flagged]);
    }

    #[test]
    fn search_criteria_from() {
        let keys = parse_search_criteria("FROM user@example.com");
        assert_eq!(keys, vec![SearchKey::From("user@example.com".into())]);
    }

    #[test]
    fn search_criteria_from_quoted() {
        let keys = parse_search_criteria("FROM \"John Doe\"");
        assert_eq!(keys, vec![SearchKey::From("John Doe".into())]);
    }

    #[test]
    fn search_criteria_to() {
        let keys = parse_search_criteria("TO admin@example.com");
        assert_eq!(keys, vec![SearchKey::To("admin@example.com".into())]);
    }

    #[test]
    fn search_criteria_subject() {
        let keys = parse_search_criteria("SUBJECT \"meeting notes\"");
        assert_eq!(keys, vec![SearchKey::Subject("meeting notes".into())]);
    }

    #[test]
    fn search_criteria_subject_unquoted() {
        let keys = parse_search_criteria("SUBJECT hello");
        assert_eq!(keys, vec![SearchKey::Subject("hello".into())]);
    }

    #[test]
    fn search_criteria_text() {
        let keys = parse_search_criteria("TEXT \"important update\"");
        assert_eq!(keys, vec![SearchKey::Text("important update".into())]);
    }

    #[test]
    fn search_criteria_body() {
        let keys = parse_search_criteria("BODY invoice");
        assert_eq!(keys, vec![SearchKey::Body("invoice".into())]);
    }

    #[test]
    fn search_criteria_since() {
        let keys = parse_search_criteria("SINCE 1-Jan-2024");
        // 1-Jan-2024 = 19723 days from epoch = 19723 * 86400
        assert_eq!(keys, vec![SearchKey::Since(19723 * 86400)]);
    }

    #[test]
    fn search_criteria_before() {
        let keys = parse_search_criteria("BEFORE 15-Mar-2024");
        assert_eq!(keys.len(), 1);
        assert!(matches!(keys[0], SearchKey::Before(_)));
    }

    #[test]
    fn search_criteria_on() {
        let keys = parse_search_criteria("ON 1-Feb-2024");
        assert_eq!(keys.len(), 1);
        assert!(matches!(keys[0], SearchKey::On(_)));
    }

    #[test]
    fn search_criteria_uid() {
        let keys = parse_search_criteria("UID 1:100");
        assert_eq!(keys, vec![SearchKey::Uid("1:100".into())]);
    }

    #[test]
    fn search_criteria_uid_single() {
        let keys = parse_search_criteria("UID 42");
        assert_eq!(keys, vec![SearchKey::Uid("42".into())]);
    }

    #[test]
    fn search_criteria_multiple_and() {
        let keys = parse_search_criteria("UNSEEN FROM user@example.com");
        assert_eq!(
            keys,
            vec![
                SearchKey::Unseen,
                SearchKey::From("user@example.com".into()),
            ]
        );
    }

    #[test]
    fn search_criteria_complex_combination() {
        let keys = parse_search_criteria("SINCE 1-Jan-2024 FROM user@example.com UNSEEN");
        assert_eq!(keys.len(), 3);
        assert!(matches!(keys[0], SearchKey::Since(_)));
        assert_eq!(keys[1], SearchKey::From("user@example.com".into()));
        assert_eq!(keys[2], SearchKey::Unseen);
    }

    #[test]
    fn search_criteria_skips_charset() {
        // CHARSET UTF-8 is commonly sent by clients, should be skipped
        let keys = parse_search_criteria("CHARSET UTF-8 UNSEEN");
        assert_eq!(keys, vec![SearchKey::Unseen]);
    }

    #[test]
    fn search_criteria_unknown_tokens_skipped() {
        let keys = parse_search_criteria("FOOBAR UNSEEN");
        assert_eq!(keys, vec![SearchKey::Unseen]);
    }

    #[test]
    fn search_criteria_date_parsing_jan() {
        let keys = parse_search_criteria("SINCE 1-Jan-1970");
        assert_eq!(keys, vec![SearchKey::Since(0)]);
    }

    #[test]
    fn search_criteria_date_parsing_various_months() {
        // verify all months parse without error
        for month in &[
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ] {
            let criteria = format!("SINCE 1-{month}-2024");
            let keys = parse_search_criteria(&criteria);
            assert_eq!(keys.len(), 1, "failed for month {month}");
            assert!(matches!(keys[0], SearchKey::Since(_)), "failed for month {month}");
        }
    }

    #[test]
    fn search_criteria_invalid_date_skipped() {
        let keys = parse_search_criteria("SINCE not-a-date");
        // invalid date is skipped, falls back to ALL
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn tokenize_quoted_strings() {
        let tokens = tokenize_search("FROM \"John Doe\" SUBJECT \"hello world\"");
        assert_eq!(
            tokens,
            vec!["FROM", "\"John Doe\"", "SUBJECT", "\"hello world\""]
        );
    }

    #[test]
    fn tokenize_no_quotes() {
        let tokens = tokenize_search("UNSEEN FLAGGED");
        assert_eq!(tokens, vec!["UNSEEN", "FLAGGED"]);
    }

    #[test]
    fn imap_date_epoch() {
        assert_eq!(parse_imap_date("1-Jan-1970"), Some(0));
    }

    #[test]
    fn imap_date_2024() {
        // 2024-01-01 = 19723 days from epoch
        let ts = parse_imap_date("1-Jan-2024").unwrap();
        assert_eq!(ts, 19723 * 86400);
    }

    #[test]
    fn imap_date_invalid() {
        assert_eq!(parse_imap_date("invalid"), None);
        assert_eq!(parse_imap_date("1-Xyz-2024"), None);
        assert_eq!(parse_imap_date("abc-Jan-2024"), None);
    }

    #[test]
    fn imap_date_leap_year() {
        // 2024 is a leap year; 1-Mar-2024 should account for 29 days in Feb
        let feb29 = parse_imap_date("29-Feb-2024");
        assert!(feb29.is_some());
        let mar1 = parse_imap_date("1-Mar-2024").unwrap();
        assert_eq!(mar1, feb29.unwrap() + 86400);
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
        if let ImapCommand::Store { sequence, action, flags } = &cmd.command {
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
        if let ImapCommand::Store { sequence, action, flags } = &cmd.command {
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
        if let ImapCommand::Fetch { sequence, attributes } = &cmd.command {
            assert_eq!(sequence, "1:5");
            assert_eq!(attributes, "(FLAGS UID ENVELOPE BODY.PEEK[HEADER])");
        } else {
            panic!("expected Fetch");
        }
    }

    #[test]
    fn search_criteria_from_at_end_no_value() {
        // FROM without a following token should be skipped, fall back to ALL
        let keys = parse_search_criteria("FROM");
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn search_criteria_to_at_end_no_value() {
        let keys = parse_search_criteria("TO");
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn search_criteria_subject_at_end_no_value() {
        let keys = parse_search_criteria("SUBJECT");
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn search_criteria_text_at_end_no_value() {
        let keys = parse_search_criteria("TEXT");
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn search_criteria_body_at_end_no_value() {
        let keys = parse_search_criteria("BODY");
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn search_criteria_uid_at_end_no_value() {
        let keys = parse_search_criteria("UID");
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn search_criteria_since_at_end_no_value() {
        let keys = parse_search_criteria("SINCE");
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn search_criteria_before_at_end_no_value() {
        let keys = parse_search_criteria("BEFORE");
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn search_criteria_on_at_end_no_value() {
        let keys = parse_search_criteria("ON");
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn search_criteria_whitespace_only_returns_all() {
        let keys = parse_search_criteria("   ");
        assert_eq!(keys, vec![SearchKey::All]);
    }

    #[test]
    fn search_criteria_multiple_flags_and_parameterized() {
        let keys = parse_search_criteria("UNSEEN UNDELETED SUBJECT test FROM sender@x.com");
        assert_eq!(keys.len(), 4);
        assert_eq!(keys[0], SearchKey::Unseen);
        assert_eq!(keys[1], SearchKey::Undeleted);
        assert_eq!(keys[2], SearchKey::Subject("test".into()));
        assert_eq!(keys[3], SearchKey::From("sender@x.com".into()));
    }

    #[test]
    fn imap_date_century_non_leap_1900() {
        // 1900 is divisible by 100 but not 400 — not a leap year
        assert!(!is_leap_year(1900));
    }

    #[test]
    fn imap_date_century_leap_2000() {
        // 2000 is divisible by 400 — leap year
        assert!(is_leap_year(2000));
    }

    #[test]
    fn imap_date_non_leap_2023() {
        assert!(!is_leap_year(2023));
    }

    #[test]
    fn imap_date_leap_2024() {
        assert!(is_leap_year(2024));
    }

    #[test]
    fn imap_date_two_digit_day() {
        let ts = parse_imap_date("15-Jun-2024");
        assert!(ts.is_some());
    }

    #[test]
    fn imap_date_missing_parts() {
        assert_eq!(parse_imap_date("1-Jan"), None);
        assert_eq!(parse_imap_date("2024"), None);
        assert_eq!(parse_imap_date(""), None);
    }

    #[test]
    fn imap_date_invalid_year() {
        assert_eq!(parse_imap_date("1-Jan-abc"), None);
    }

    #[test]
    fn imap_date_dec_31() {
        // last day of a non-leap year
        let dec31 = parse_imap_date("31-Dec-2023").unwrap();
        let jan1_next = parse_imap_date("1-Jan-2024").unwrap();
        assert_eq!(jan1_next - dec31, 86400);
    }

    #[test]
    fn tokenize_search_empty() {
        let tokens = tokenize_search("");
        assert!(tokens.is_empty());
    }

    #[test]
    fn tokenize_search_only_spaces() {
        let tokens = tokenize_search("   ");
        assert!(tokens.is_empty());
    }

    #[test]
    fn tokenize_search_multiple_spaces_between_tokens() {
        let tokens = tokenize_search("FROM   user@example.com");
        assert_eq!(tokens, vec!["FROM", "user@example.com"]);
    }

    #[test]
    fn tokenize_search_unclosed_quote_treated_as_unquoted() {
        // unclosed quote: remaining text pushed as-is
        let tokens = tokenize_search("FROM \"unclosed");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0], "FROM");
        // unclosed quote leaves the text in current buffer
        assert_eq!(tokens[1], "unclosed");
    }

    #[test]
    fn unquote_no_quotes() {
        assert_eq!(unquote("hello"), "hello");
    }

    #[test]
    fn unquote_with_quotes() {
        assert_eq!(unquote("\"hello\""), "hello");
    }

    #[test]
    fn unquote_single_char_quoted() {
        assert_eq!(unquote("\"x\""), "x");
    }

    #[test]
    fn unquote_empty_quoted() {
        assert_eq!(unquote("\"\""), "");
    }

    #[test]
    fn unquote_only_one_quote() {
        // single quote at start only — not matching pair
        assert_eq!(unquote("\"hello"), "\"hello");
    }

    #[test]
    fn parse_error_equality() {
        assert_eq!(ParseError::EmptyInput, ParseError::EmptyInput);
        assert_eq!(ParseError::MissingTag, ParseError::MissingTag);
        assert_ne!(ParseError::EmptyInput, ParseError::MissingTag);
        assert_eq!(
            ParseError::UnknownCommand("X".into()),
            ParseError::UnknownCommand("X".into())
        );
        assert_ne!(
            ParseError::UnknownCommand("X".into()),
            ParseError::UnknownCommand("Y".into())
        );
    }

    #[test]
    fn tagged_command_clone_and_eq() {
        let cmd = parse_command("a001 NOOP").unwrap();
        let cloned = cmd.clone();
        assert_eq!(cmd, cloned);
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
