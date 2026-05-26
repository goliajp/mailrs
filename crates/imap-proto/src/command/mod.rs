//! IMAP4rev1 command AST + parser entry points.
//!
//! Public surface:
//! - [`parse_command`] — single-line IMAP command parser (in [`parse`])
//! - [`ImapCommand`] / [`TaggedCommand`] / [`ParseError`] — command AST + errors
//! - [`SearchKey`] / [`parse_search_criteria`] — SEARCH criteria parser (in [`search`])

mod parse;
mod search;

pub use parse::parse_command;
pub use search::{SearchKey, parse_search_criteria};

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

    // --- unquote helper ---

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
}
