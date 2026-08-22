#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

#[cfg(feature = "net")]
mod net;
#[cfg(feature = "net")]
pub use net::{Error as NetError, Session, Tls};

/// One line from the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// `+OK …`
    Ok(String),
    /// `-ERR …`
    Err(String),
    /// Anything else — every line of a multi-line response.
    Data(String),
}

/// One message's session number and its durable identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uid {
    /// Its number **in this session**. Renumbers when anything is
    /// deleted, so it is never remembered between syncs.
    pub number: u32,
    /// The `UIDL` string, which is stable for as long as the message
    /// exists. This is the only thing worth storing.
    pub uid: String,
}

/// Read one line.
pub fn parse_line(line: &str) -> Line {
    let trimmed = line.trim_end_matches(['\r', '\n']);
    match trimmed.split_once(' ') {
        Some(("+OK", rest)) => Line::Ok(rest.to_string()),
        Some(("-ERR", rest)) => Line::Err(rest.to_string()),
        _ if trimmed == "+OK" => Line::Ok(String::new()),
        _ if trimmed == "-ERR" => Line::Err(String::new()),
        _ => Line::Data(trimmed.to_string()),
    }
}

/// Whether an `-ERR` means the credential was refused.
///
/// The one failure that must not be retried on a timer: waiting cannot
/// fix a password that changed, and some providers count the attempts
/// and lock the account.
pub fn is_authentication_failure(line: &str) -> bool {
    let up = line.to_ascii_uppercase();
    up.contains("[AUTH]")
        || up.contains("AUTHENTICATION FAILED")
        || up.contains("INVALID PASSWORD")
        || up.contains("LOGIN FAILED")
        || up.contains("AUTHORIZATION FAILED")
}

/// Whether an `-ERR` means the server has no `UIDL` **at all**.
///
/// Such a server cannot be deduplicated, and the honest answer is to
/// say so when the account is set up rather than re-download the
/// mailbox on every sync for as long as it exists.
///
/// **Not every `-ERR` to `UIDL` means this.** A locked mailbox answers
/// `-ERR` too, and it is temporary — reading it as "no UIDL" marks the
/// account permanently broken over a lock that would have cleared in a
/// minute, and only a person can undo that. POP3 has no response
/// codes, so the words are all there is: a server that does not know
/// the command says so.
pub fn no_uidl(line: &str) -> bool {
    let Line::Err(rest) = parse_line(line) else {
        return false;
    };
    let up = rest.to_ascii_uppercase();
    up.contains("UNKNOWN COMMAND")
        || up.contains("NOT IMPLEMENTED")
        || up.contains("NOT SUPPORTED")
        || up.contains("INVALID COMMAND")
        || up.contains("UNIMPLEMENTED")
        || up.contains("COMMAND NOT")
}

/// Read a `UIDL` listing.
///
/// A line this cannot read is skipped rather than guessed at: a wrong
/// uid is worse than a missing one, because it makes one message
/// permanently invisible instead of downloading it twice.
pub fn parse_uidl(lines: &[&str]) -> Vec<Uid> {
    lines
        .iter()
        .filter_map(|l| {
            // The first space only — a uid may contain anything
            // printable, and some servers put spaces in them.
            let (number, uid) = l.trim().split_once(' ')?;
            let uid = uid.trim();
            if uid.is_empty() {
                return None;
            }
            Some(Uid {
                number: number.parse().ok()?,
                uid: uid.to_string(),
            })
        })
        .collect()
}

/// The messages on the server that are not already held.
pub fn not_yet_held<'a>(on_server: &'a [Uid], held: &[String]) -> Vec<&'a Uid> {
    on_server
        .iter()
        .filter(|u| !held.iter().any(|h| h == &u.uid))
        .collect()
}
