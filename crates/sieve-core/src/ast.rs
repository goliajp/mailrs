//! RFC 5228 §3-4 AST.

/// One Sieve command — identifier + args + optional nested block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// Command identifier (e.g. `"if"`, `"keep"`, `"fileinto"`).
    pub name: String,
    /// Positional + tagged + test arguments, in source order.
    pub args: Vec<Argument>,
    /// Block body (`{ … }`) — empty for non-block commands.
    pub block: Vec<Command>,
}

/// Argument to a command or a nested test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Argument {
    /// `:identifier` tag.
    Tag(String),
    /// Numeric literal (with K/M/G already applied).
    Number(u64),
    /// String literal — quoted or multi-line.
    String(String),
    /// Bracketed string list `[ "a", "b" ]`.
    StringList(Vec<String>),
    /// Nested test (`header :is "Subject" "spam"`,
    /// `allof(...)`, `anyof(...)`, `not test`, …).
    Test(Test),
    /// Nested test list (`allof(t1, t2)`, `anyof(t1, t2)`).
    TestList(Vec<Test>),
}

/// One test expression. The match-types + address-parts get
/// extracted into the dedicated fields so the evaluator doesn't
/// scan the arg list every time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Test {
    /// Test identifier (`"header"`, `"address"`, `"size"`, …).
    pub name: String,
    /// Tag arguments (`:is`, `:domain`, `:localpart`, …) in
    /// source order.
    pub tags: Vec<String>,
    /// Positional args (numeric / string / string-list).
    pub args: Vec<Argument>,
    /// Nested tests (for `allof`, `anyof`, `not`).
    pub children: Vec<Test>,
}

/// Standardised match-type for the evaluator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchType {
    /// `:is` — exact case-insensitive match.
    Is,
    /// `:contains` — substring case-insensitive match.
    Contains,
    /// `:matches` — wildcard match (`*` / `?`).
    Matches,
}

impl MatchType {
    /// Pick the first match-type tag from a `tags` slice, defaulting to `:is`.
    pub fn from_tags(tags: &[String]) -> Self {
        for t in tags {
            match t.as_str() {
                "is" => return Self::Is,
                "contains" => return Self::Contains,
                "matches" => return Self::Matches,
                _ => {}
            }
        }
        Self::Is
    }
}

/// One action the evaluator emits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Keep the message in the default mailbox (the implicit
    /// default when no action fires). The `flags` vector carries
    /// any IMAP flags set by RFC 5232 `imap4flags` extension
    /// (empty when the extension is not used).
    Keep {
        /// IMAP flags to attach to the kept copy (RFC 5232).
        flags: Vec<String>,
    },
    /// Discard the message — drop without notice.
    Discard,
    /// File into a named mailbox (RFC 5228 `fileinto` ext) with
    /// optional IMAP flags (RFC 5232 `imap4flags` ext — empty
    /// when the extension is not used).
    FileInto {
        /// Destination mailbox name.
        mailbox: String,
        /// IMAP flags to attach to the filed copy (RFC 5232).
        flags: Vec<String>,
    },
    /// Forward / redirect to another address.
    Redirect(String),
    /// Reject with the given reason string.
    Reject(String),
    /// RFC 5230 `vacation` — generate an automatic reply. The
    /// stateful parts (dedup window, recipient detection, reply
    /// message build) are the caller's job; this engine only
    /// surfaces the parsed action.
    Vacation(VacationAction),
}

/// RFC 5230 `vacation` action — everything the caller needs to
/// generate the auto-reply.
///
/// Fields with `Option` default to "use the server-defined
/// value" (per RFC 5230 §4.1–4.5). `addresses` and `mime` default
/// to `Vec::new()` / `false`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VacationAction {
    /// The auto-reply body (positional argument — required).
    pub reason: String,
    /// `:days <n>` or `:seconds <n>` — the dedup window.
    /// `None` = server default (RFC 5230 §4.1 hints at 7 days).
    pub period: Option<VacationPeriod>,
    /// `:subject <s>` — overrides the default `Auto:`-prefixed
    /// Subject (RFC 5230 §4.2).
    pub subject: Option<String>,
    /// `:from <addr>` — overrides the default `From:` of the auto-
    /// reply (RFC 5230 §4.3); must be a path the mailbox owner can
    /// legitimately use.
    pub from: Option<String>,
    /// `:addresses [<a>, <b>, …]` — alternate recipient addresses
    /// the user may be addressed at; the auto-reply is only sent
    /// if one of these matches the incoming envelope-to (RFC 5230
    /// §4.4).
    pub addresses: Vec<String>,
    /// `:mime` — when true the `reason` is a full RFC 2822 MIME
    /// entity; otherwise it's plain text (RFC 5230 §4.5).
    pub mime: bool,
    /// `:handle <h>` — handle for the dedup index (RFC 5230 §4.6);
    /// distinct reasons sharing one handle are deduplicated as one.
    pub handle: Option<String>,
}

/// The `:days` / `:seconds` window on a `vacation` action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VacationPeriod {
    /// `:days <n>` — RFC 5230 §4.1.
    Days(u64),
    /// `:seconds <n>` — RFC 6131 extension (sub-day windows).
    Seconds(u64),
}
