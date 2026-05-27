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
    /// default when no action fires).
    Keep,
    /// Discard the message — drop without notice.
    Discard,
    /// File into a named mailbox (RFC 5228 `fileinto` ext).
    FileInto(String),
    /// Forward / redirect to another address.
    Redirect(String),
    /// Reject with the given reason string.
    Reject(String),
}
