//! The least IMAP client an import needs: sign in, list the folders, open
//! one read-only, list its messages, fetch one without marking it read.
//!
//! The server at the other end is someone else's, so everything it sends
//! is bounded before it is held: a line by the codec, a literal by its
//! declared size (refused before a byte of it is read), every command's
//! whole answer by a byte budget and a deadline. A server that answers
//! forever fails the command rather than filling memory.

use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use mailrs_imap_codec::{ImapCodec, ImapInput};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

/// What one connection may receive.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// The largest message fetched, and so the largest literal accepted.
    pub max_message: u32,
    /// Everything one command's answer may add up to.
    pub max_command_bytes: usize,
    /// How long one command may take, answer included.
    pub command_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_message: mailrs_core_api::method::sent::MAX_MESSAGE_BYTES as u32,
            // the largest message, and room for a folder list or a message
            // list of a large mailbox
            max_command_bytes: mailrs_core_api::method::sent::MAX_MESSAGE_BYTES + 32 * 1024 * 1024,
            command_timeout: Duration::from_secs(300),
        }
    }
}

/// Why a command failed.
#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    /// The server declared a literal larger than [`Limits::max_message`].
    TooLarge(u64),
    /// The answer outgrew [`Limits::max_command_bytes`].
    Overrun,
    Timeout,
    /// The server refused (`NO`, `BAD`, `BYE`), with what it said.
    Refused(String),
    /// The answer did not parse.
    Protocol(String),
}

/// What the server said, cut short and stripped of control characters: it
/// reaches the settings page as plain text.
pub fn shown(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(200).collect()
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "connection failed: {e}"),
            Error::TooLarge(n) => write!(f, "the server sent a {n}-byte literal"),
            Error::Overrun => f.write_str("the server's answer was too long"),
            Error::Timeout => f.write_str("the server did not answer in time"),
            Error::Refused(t) => write!(f, "the server refused: {}", shown(t)),
            Error::Protocol(t) => write!(f, "the server's answer did not parse: {}", shown(t)),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

/// One untagged answer: its text, with each literal's place marked `\0`,
/// and the literals in order.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Untagged {
    pub text: String,
    pub literals: Vec<Vec<u8>>,
}

/// A connection after the greeting.
pub struct Client<S> {
    io: Framed<S, ImapCodec>,
    tag: u32,
    limits: Limits,
    /// A line read past the end of a literal-carrying answer, for the next.
    pending: Option<String>,
}

/// An untagged answer, a continuation request, or one of this client's
/// tagged ones.
fn starts_an_answer(line: &str) -> bool {
    if line.starts_with("* ") || line.starts_with('+') {
        return true;
    }
    line.strip_prefix('i')
        .and_then(|r| r.split_once(' '))
        .is_some_and(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// `{N}` or `{N+}` ending a line: the literal that follows it.
fn literal_size(line: &str) -> Option<u64> {
    let open = line.rfind('{')?;
    let inner = line[open + 1..].strip_suffix('}')?;
    inner.trim_end_matches('+').parse().ok()
}

impl<S: AsyncRead + AsyncWrite + Unpin> Client<S> {
    /// Read the greeting, unless whoever opened the connection already did
    /// (to ask for STARTTLS before handing it over).
    pub async fn open(stream: S, limits: Limits, greeted: bool) -> Result<Self, Error> {
        let codec = ImapCodec::new().with_max_literal(limits.max_message);
        let mut c = Self {
            io: Framed::new(stream, codec),
            tag: 0,
            limits,
            pending: None,
        };
        if !greeted {
            let mut budget = limits.max_command_bytes;
            let line = tokio::time::timeout(limits.command_timeout, c.line(&mut budget))
                .await
                .map_err(|_| Error::Timeout)??;
            match line.split_whitespace().nth(1) {
                Some(s) if s.eq_ignore_ascii_case("OK") || s.eq_ignore_ascii_case("PREAUTH") => {}
                _ => return Err(Error::Refused(line)),
            }
        }
        Ok(c)
    }

    async fn line(&mut self, budget: &mut usize) -> Result<String, Error> {
        if let Some(l) = self.pending.take() {
            return Ok(l);
        }
        match self.io.next().await {
            Some(Ok(ImapInput::Line(l))) => {
                *budget = budget.checked_sub(l.len() + 2).ok_or(Error::Overrun)?;
                Ok(l)
            }
            Some(Ok(ImapInput::LiteralData(_))) => {
                Err(Error::Protocol("unexpected literal".into()))
            }
            Some(Err(e)) => Err(Error::Io(e)),
            None => Err(Error::Io(std::io::ErrorKind::UnexpectedEof.into())),
        }
    }

    /// One whole answer line, with any literals it carries read in.
    async fn answer(&mut self, budget: &mut usize) -> Result<Untagged, Error> {
        let mut out = Untagged::default();
        let mut line = self.line(budget).await?;
        while let Some(size) = literal_size(&line) {
            if size > u64::from(self.limits.max_message) {
                return Err(Error::TooLarge(size));
            }
            let open = line.rfind('{').expect("literal_size found it");
            out.text.push_str(&line[..open]);
            out.text.push('\0');
            *budget = budget.checked_sub(size as usize).ok_or(Error::Overrun)?;
            self.io.codec_mut().expect_literal(size as u32);
            match self.io.next().await {
                Some(Ok(ImapInput::LiteralData(d))) => out.literals.push(d),
                Some(Ok(ImapInput::Line(_))) => {
                    return Err(Error::Protocol("a line where a literal was due".into()));
                }
                Some(Err(e)) => return Err(Error::Io(e)),
                None => return Err(Error::Io(std::io::ErrorKind::UnexpectedEof.into())),
            }
            // The codec eats the CRLF after a literal when it has already
            // arrived, so an answer ending in a literal is followed either
            // by an empty line or straight by the next answer. What
            // continues an answer after a literal never starts like one.
            let next = self.line(budget).await?;
            if next.is_empty() {
                return Ok(out);
            }
            if starts_an_answer(&next) {
                self.pending = Some(next);
                return Ok(out);
            }
            line = next;
        }
        out.text.push_str(&line);
        Ok(out)
    }

    async fn send(&mut self, bytes: Vec<u8>) -> Result<(), Error> {
        self.io.send(bytes).await.map_err(Error::Io)
    }

    /// Send `parts` as one command — text as written, literals as
    /// synchronising literals — and collect the untagged answers up to its
    /// tagged one.
    async fn run(&mut self, parts: &[Part<'_>]) -> Result<Vec<Untagged>, Error> {
        let limits = self.limits;
        tokio::time::timeout(limits.command_timeout, self.run_unbounded(parts))
            .await
            .map_err(|_| Error::Timeout)?
    }

    async fn run_unbounded(&mut self, parts: &[Part<'_>]) -> Result<Vec<Untagged>, Error> {
        self.tag += 1;
        let tag = format!("i{}", self.tag);
        let mut budget = self.limits.max_command_bytes;
        let mut pending = format!("{tag} ").into_bytes();
        for part in parts {
            match part {
                Part::Text(t) => pending.extend_from_slice(t.as_bytes()),
                Part::Literal(bytes) => {
                    pending.extend_from_slice(format!("{{{}}}\r\n", bytes.len()).as_bytes());
                    self.send(std::mem::take(&mut pending)).await?;
                    let go = self.line(&mut budget).await?;
                    if !go.starts_with('+') {
                        return Err(Error::Refused(go));
                    }
                    pending.extend_from_slice(bytes);
                }
            }
        }
        pending.extend_from_slice(b"\r\n");
        self.send(pending).await?;
        let mut untagged = Vec::new();
        loop {
            let a = self.answer(&mut budget).await?;
            if let Some(rest) = a.text.strip_prefix(&format!("{tag} ")) {
                let status = rest.split_whitespace().next().unwrap_or("");
                return match status.eq_ignore_ascii_case("OK") {
                    true => Ok(untagged),
                    false => Err(Error::Refused(rest.to_string())),
                };
            }
            if a.text.starts_with("* BYE") {
                return Err(Error::Refused(a.text));
            }
            untagged.push(a);
        }
    }

    /// `LOGIN`, each value quoted, or sent as a literal when it holds a
    /// character a quoted string cannot.
    pub async fn login(&mut self, user: &str, password: &str) -> Result<(), Error> {
        let (u, p) = (Arg::of(user), Arg::of(password));
        self.run(&[Part::Text("LOGIN "), u.part(), Part::Text(" "), p.part()])
            .await
            .map(|_| ())
    }

    /// Every folder, with its attributes.
    pub async fn list(&mut self) -> Result<Vec<Folder>, Error> {
        let answers = self.run(&[Part::Text("LIST \"\" \"*\"")]).await?;
        Ok(answers.iter().filter_map(parse_list).collect())
    }

    /// Open `folder` read-only.
    pub async fn examine(&mut self, folder: &str) -> Result<(), Error> {
        let name = Arg::of(folder);
        self.run(&[Part::Text("EXAMINE "), name.part()])
            .await
            .map(|_| ())
    }

    /// The messages of the open folder from `from_uid` on, ascending.
    pub async fn messages_from(&mut self, from_uid: u32) -> Result<Vec<Listed>, Error> {
        let cmd = format!("UID FETCH {from_uid}:* (UID RFC822.SIZE FLAGS)");
        let answers = self.run(&[Part::Text(&cmd)]).await?;
        let mut out: Vec<Listed> = answers
            .iter()
            .filter_map(parse_listed)
            // `n:*` past the last message names the last message
            .filter(|l| l.uid >= from_uid)
            .collect();
        out.sort_by_key(|l| l.uid);
        out.dedup_by_key(|l| l.uid);
        Ok(out)
    }

    /// One message's bytes, leaving it unread at the source.
    pub async fn fetch(&mut self, uid: u32) -> Result<Vec<u8>, Error> {
        let cmd = format!("UID FETCH {uid} (UID BODY.PEEK[])");
        let answers = self.run(&[Part::Text(&cmd)]).await?;
        answers
            .into_iter()
            .find(|a| parse_uid(&a.text) == Some(uid))
            .and_then(|a| a.literals.into_iter().next())
            .ok_or_else(|| Error::Protocol(format!("no body for UID {uid}")))
    }

    /// Say goodbye; the answer does not matter.
    pub async fn logout(&mut self) {
        let _ = self.run(&[Part::Text("LOGOUT")]).await;
    }
}

enum Part<'a> {
    Text(&'a str),
    Literal(&'a [u8]),
}

/// A string argument: quoted when every character may be, else a literal.
enum Arg<'a> {
    Quoted(String),
    Literal(&'a str),
}

impl<'a> Arg<'a> {
    fn of(s: &'a str) -> Self {
        let plain = s.chars().all(|c| c.is_ascii() && !c.is_ascii_control());
        match plain {
            true => Arg::Quoted(format!(
                "\"{}\"",
                s.replace('\\', "\\\\").replace('"', "\\\"")
            )),
            false => Arg::Literal(s),
        }
    }

    fn part(&self) -> Part<'_> {
        match self {
            Arg::Quoted(q) => Part::Text(q),
            Arg::Literal(s) => Part::Literal(s.as_bytes()),
        }
    }
}

/// A folder as `LIST` names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub name: String,
    /// Attributes, as written: `\Sent`, `\Noselect`, …
    pub flags: Vec<String>,
}

/// A message as the listing names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Listed {
    pub uid: u32,
    pub size: u64,
    pub seen: bool,
}

/// `* LIST (\HasNoChildren \Sent) "/" "Sent Items"`; the name may be a
/// quoted string, an atom, or a literal.
fn parse_list(a: &Untagged) -> Option<Folder> {
    let rest = a.text.strip_prefix("* LIST ")?;
    let open = rest.find('(')?;
    let close = rest[open..].find(')')? + open;
    let flags = rest[open + 1..close]
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let after = rest[close + 1..].trim_start();
    // the delimiter: a quoted character or NIL
    let after = match after.strip_prefix("NIL") {
        Some(r) => r,
        None => {
            let (_, r) = quoted(after)?;
            r
        }
    }
    .trim_start();
    let name = match after.chars().next()? {
        '\0' => String::from_utf8(a.literals.first()?.clone()).ok()?,
        '"' => quoted(after)?.0,
        _ => after.trim().to_string(),
    };
    Some(Folder { name, flags })
}

/// A quoted string at the start of `s`, unescaped, and what follows it.
fn quoted(s: &str) -> Option<(String, &str)> {
    let mut chars = s.strip_prefix('"')?.char_indices();
    let mut out = String::new();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => out.push(chars.next()?.1),
            '"' => return Some((out, &s[i + 2..])),
            _ => out.push(c),
        }
    }
    None
}

fn number_after(text: &str, key: &str) -> Option<u64> {
    let upper = text.to_ascii_uppercase();
    let at = upper.find(&format!("{key} "))? + key.len() + 1;
    let digits: String = text[at..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

fn parse_uid(text: &str) -> Option<u32> {
    if !text.starts_with("* ") || !text.to_ascii_uppercase().contains(" FETCH ") {
        return None;
    }
    number_after(text, "UID").and_then(|n| u32::try_from(n).ok())
}

/// `* 12 FETCH (UID 345 RFC822.SIZE 1234 FLAGS (\Seen))`
fn parse_listed(a: &Untagged) -> Option<Listed> {
    let uid = parse_uid(&a.text)?;
    let upper = a.text.to_ascii_uppercase();
    let flags = upper
        .find("FLAGS (")
        .map(|at| &upper[at + 7..])
        .and_then(|r| r.split(')').next())
        .unwrap_or("");
    Some(Listed {
        uid,
        size: number_after(&a.text, "RFC822.SIZE").unwrap_or(0),
        seen: flags.split_whitespace().any(|f| f == "\\SEEN"),
    })
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
