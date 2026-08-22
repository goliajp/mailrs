//! Reading what a server said.
//!
//! Deliberately tolerant. A parser in a sync worker meets every server
//! anybody has ever run, and a line it cannot read must be a line it
//! ignores — a panic here takes every other account down with it, and
//! an error return that the caller must handle for each of a hundred
//! untagged lines is an error return the caller will start ignoring.
//!
//! So: `Option`, `None` for anything unrecognised, and no panics on any
//! input. The tests feed it truncated, oversized and non-ASCII lines.

/// An untagged response, in the shapes a client acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Untagged {
    /// `* 231 EXISTS`
    Exists(u32),
    /// `* OK [UIDVALIDITY 1234567890]`
    UidValidity(u32),
    /// `* OK [UIDNEXT 4392]`
    UidNext(u32),
    /// `* 12 FETCH (...)`
    Fetch(Fetch),
    /// `* LIST (...) "/" "INBOX"`
    List(List),
}

/// What one `FETCH` said about one message.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Fetch {
    /// Its uid, when the server included one.
    pub uid: Option<u32>,
    /// `\Seen`.
    pub seen: bool,
    /// `\Answered`.
    pub answered: bool,
    /// `\Flagged`.
    pub flagged: bool,
    /// `\Deleted`.
    pub deleted: bool,
    /// `\Draft`.
    pub draft: bool,
    /// `RFC822.SIZE`, when given.
    pub size: Option<u64>,
    /// The `{n}` literal length announced at the end of the line, which
    /// is how many bytes of body follow.
    pub literal_len: Option<u64>,
}

/// One folder from a `LIST`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct List {
    /// Its full name.
    pub name: String,
    /// The hierarchy separator, `NIL` for a flat namespace.
    pub delimiter: Option<String>,
    /// `\Noselect` — it holds other folders and cannot be opened.
    /// Named for what it forbids rather than as `selectable`, so a
    /// default of `false` cannot read as "cannot be opened".
    pub selectable_is_false: bool,
    /// `\Sent`, by the RFC 6154 attribute.
    pub is_sent: bool,
    /// `\Drafts`.
    pub is_drafts: bool,
    /// `\Trash`.
    pub is_trash: bool,
    /// `\Junk`.
    pub is_junk: bool,
    /// `\All` — a view holding a copy of everything, which is not a
    /// folder to sync.
    pub is_all: bool,
}

/// One line from the server, if it is one a client acts on.
///
/// Tagged responses (`a001 OK …`) return `None`: they answer a command
/// the caller sent and it is the caller that knows which.
pub fn parse_line(line: &str) -> Option<Untagged> {
    let rest = line.strip_prefix("* ")?;
    if let Some(v) = bracketed_number(rest, "UIDVALIDITY") {
        return Some(Untagged::UidValidity(v));
    }
    if let Some(v) = bracketed_number(rest, "UIDNEXT") {
        return Some(Untagged::UidNext(v));
    }
    if let Some(l) = rest.strip_prefix("LIST ") {
        return parse_list(l).map(Untagged::List);
    }
    // `* <n> EXISTS` and `* <n> FETCH (...)` both begin with a number.
    let (first, tail) = rest.split_once(' ')?;
    if tail == "EXISTS" {
        return first.parse().ok().map(Untagged::Exists);
    }
    if let Some(body) = tail.strip_prefix("FETCH ") {
        return Some(Untagged::Fetch(parse_fetch(body)));
    }
    None
}

/// Whether a tagged `NO` means the credential was refused.
///
/// The response codes are RFC 5530's. This is the one failure that
/// must not be retried on a timer: waiting cannot fix it, and some
/// providers count the attempts.
pub fn is_authentication_failure(line: &str) -> bool {
    let up = line.to_ascii_uppercase();
    up.contains("[AUTHENTICATIONFAILED]")
        || up.contains("[AUTHORIZATIONFAILED]")
        || up.contains("[EXPIRED]")
        || up.contains("[PRIVACYREQUIRED]")
}

fn bracketed_number(rest: &str, name: &str) -> Option<u32> {
    let at = rest.find(&format!("[{name} "))?;
    let after = &rest[at + name.len() + 2..];
    let end = after.find(']')?;
    after[..end].trim().parse().ok()
}

fn parse_fetch(body: &str) -> Fetch {
    // Matched with the leading backslash so a folder named "Seen" in
    // some other part of the line cannot set a flag.
    let flags = between(body, "FLAGS (", ")").unwrap_or_default();
    Fetch {
        uid: word_after(body, "UID").and_then(|v| v.parse().ok()),
        size: word_after(body, "RFC822.SIZE").and_then(|v| v.parse().ok()),
        seen: flags.contains("\\Seen"),
        answered: flags.contains("\\Answered"),
        flagged: flags.contains("\\Flagged"),
        deleted: flags.contains("\\Deleted"),
        draft: flags.contains("\\Draft"),
        literal_len: between(body, "{", "}").and_then(|n| n.parse().ok()),
    }
}

fn parse_list(rest: &str) -> Option<List> {
    let attrs = between(rest, "(", ")").unwrap_or_default().to_string();
    let after = &rest[rest.find(')').map(|i| i + 1)?..];
    let mut parts = after.trim().splitn(2, ' ');
    let delim_raw = parts.next()?.trim();
    let name_raw = parts.next()?.trim();
    if name_raw.is_empty() {
        return None;
    }
    let low = attrs.to_ascii_lowercase();
    Some(List {
        name: unquote(name_raw),
        delimiter: match delim_raw {
            "NIL" | "nil" => None,
            v => Some(unquote(v)),
        },
        selectable_is_false: low.contains("\\noselect"),
        is_sent: low.contains("\\sent"),
        is_drafts: low.contains("\\drafts"),
        is_trash: low.contains("\\trash"),
        is_junk: low.contains("\\junk"),
        is_all: low.contains("\\all"),
    })
}

fn unquote(v: &str) -> String {
    v.trim().trim_matches('"').to_string()
}

fn between<'a>(haystack: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = haystack.find(open)? + open.len();
    let end = haystack[start..].find(close)? + start;
    Some(&haystack[start..end])
}

fn word_after<'a>(haystack: &'a str, key: &str) -> Option<&'a str> {
    let at = haystack.find(key)? + key.len();
    let rest = haystack[at..].trim_start();
    Some(rest.split([' ', ')']).next().unwrap_or(rest))
}
