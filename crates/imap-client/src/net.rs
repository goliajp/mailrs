//! The socket half: connect, log in, list, fetch.
//!
//! Behind the `net` feature, so the parsing and the uid bookkeeping
//! stay usable — and testable — without a TLS stack.
//!
//! Deliberately small. It speaks the handful of commands a one-way sync
//! needs and nothing else: no IDLE, no APPEND, no server-side search.
//! Each of those is a feature with its own decisions, and a client that
//! grows them by accident grows them badly.

use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use crate::{Untagged, is_authentication_failure, parse_line};

/// How a connection is protected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tls {
    /// TLS from the first byte — port 993.
    Implicit,
    /// Plain, upgraded with `STARTTLS`.
    StartTls,
    /// None. Some intranet servers still have none.
    None,
}

/// What went wrong, in words that reach the row and then the screen.
#[derive(Debug)]
pub enum Error {
    /// Could not reach the server at all.
    Connect(String),
    /// TLS did not come up.
    Tls(String),
    /// The credential was refused. **Not retried on a timer** — waiting
    /// cannot fix it, and some providers count the attempts.
    Auth(String),
    /// The server said no to something else.
    Server(String),
    /// The connection broke mid-conversation.
    Io(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connect(m) => write!(f, "cannot reach the server: {m}"),
            Self::Tls(m) => write!(f, "TLS failed: {m}"),
            Self::Auth(m) => write!(f, "the password was refused: {m}"),
            Self::Server(m) => write!(f, "the server refused: {m}"),
            Self::Io(m) => write!(f, "the connection broke: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    /// Whether waiting could ever help.
    pub fn is_permanent(&self) -> bool {
        matches!(self, Self::Auth(_))
    }
}

type Stream = Box<dyn Rw>;

/// Either half of the connection, before and after `STARTTLS`.
pub trait Rw: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Rw for T {}

/// One IMAP conversation.
pub struct Session {
    io: BufReader<Stream>,
    tag: u32,
}

impl Session {
    /// Open a connection and greet.
    pub async fn connect(host: &str, port: u16, tls: Tls) -> Result<Self, Error> {
        let tcp = TcpStream::connect((host, port))
            .await
            .map_err(|e| Error::Connect(e.to_string()))?;
        let stream: Stream = match tls {
            Tls::Implicit => Box::new(wrap_tls(tcp, host).await?),
            Tls::None | Tls::StartTls => Box::new(tcp),
        };
        let mut s = Self {
            io: BufReader::new(stream),
            tag: 0,
        };
        // The greeting. A server that will not greet is not a server we
        // can use, and saying so here is clearer than failing at LOGIN.
        let greeting = s.read_line().await?;
        if !greeting.starts_with("* OK") && !greeting.starts_with("* PREAUTH") {
            return Err(Error::Server(greeting.trim().to_string()));
        }
        if tls == Tls::StartTls {
            s.upgrade(host).await?;
        }
        Ok(s)
    }

    async fn upgrade(&mut self, host: &str) -> Result<(), Error> {
        let (_, lines) = self.command("STARTTLS").await?;
        let _ = lines;
        // Rebuild over the now-encrypted socket. The buffered reader is
        // dropped with whatever it had read, which is nothing: the
        // tagged OK is the last byte before the handshake by
        // definition, and reading further would be reading plaintext
        // the server has not sent.
        let inner = std::mem::replace(&mut self.io, BufReader::new(Box::new(NullIo)));
        let tcp = inner.into_inner();
        self.io = BufReader::new(Box::new(wrap_tls(tcp, host).await?));
        Ok(())
    }

    /// `LOGIN`, quoted so a password with a space or a quote works.
    ///
    /// Passwords with `"` in them are common in generated app
    /// passwords, and an unquoted LOGIN turns one into a syntax error
    /// that reads as "wrong password".
    pub async fn login(&mut self, user: &str, secret: &str) -> Result<(), Error> {
        let cmd = format!("LOGIN {} {}", quoted(user), quoted(secret));
        match self.command(&cmd).await {
            Ok(_) => Ok(()),
            Err(Error::Server(m)) if is_authentication_failure(&m) => Err(Error::Auth(m)),
            // A bare `NO` from LOGIN is a refused credential too: not
            // every server sends a response code, and treating those as
            // retryable hammers the provider with the wrong password.
            Err(Error::Server(m)) => Err(Error::Auth(m)),
            Err(e) => Err(e),
        }
    }

    /// `AUTHENTICATE XOAUTH2` — what Gmail and Outlook require.
    pub async fn authenticate_xoauth2(&mut self, user: &str, token: &str) -> Result<(), Error> {
        use base64::Engine as _;
        let blob = format!("user={user}\x01auth=Bearer {token}\x01\x01");
        let b64 = base64::engine::general_purpose::STANDARD.encode(blob);
        match self.command(&format!("AUTHENTICATE XOAUTH2 {b64}")).await {
            Ok(_) => Ok(()),
            // The server answers a failed XOAUTH2 with a base64 error
            // document and waits for an empty line before the tagged
            // NO. Sending it keeps the connection in a state we can
            // close cleanly rather than leaving the server waiting.
            Err(Error::Server(m)) => {
                let _ = self.raw("\r\n").await;
                Err(Error::Auth(m))
            }
            Err(e) => Err(e),
        }
    }

    /// Every folder the account has.
    pub async fn list(&mut self) -> Result<Vec<crate::List>, Error> {
        let (_, lines) = self.command("LIST \"\" \"*\"").await?;
        Ok(lines
            .iter()
            .filter_map(|l| match parse_line(l) {
                Some(Untagged::List(f)) => Some(f),
                _ => None,
            })
            .collect())
    }

    /// Open a folder, and report what the server said about it.
    pub async fn select(&mut self, folder: &str) -> Result<crate::FolderState, Error> {
        let (_, lines) = self.command(&format!("SELECT {}", quoted(folder))).await?;
        let mut state = crate::FolderState::default();
        for l in &lines {
            if let Some(u) = parse_line(l) {
                state.apply(&u);
            }
        }
        Ok(state)
    }

    /// Mark messages read on the server.
    ///
    /// `+FLAGS.SILENT` rather than `+FLAGS`: the untagged FETCH the
    /// non-silent form sends back would have to be read and thrown
    /// away, and anything left unread in the buffer desynchronises the
    /// next command's replies.
    ///
    /// A uid the server no longer holds is not an error in IMAP — the
    /// STORE simply affects nothing — which is the behaviour wanted
    /// here: a note for a message somebody deleted at the other end
    /// should disappear, not retry forever.
    pub async fn store_seen(&mut self, uids: &[u32]) -> Result<(), Error> {
        if uids.is_empty() {
            return Ok(());
        }
        let set = uids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let tag = self.next_tag();
        self.raw(&format!("{tag} UID STORE {set} +FLAGS.SILENT (\\Seen)\r\n"))
            .await?;
        loop {
            let line = self.read_line().await?;
            if line.starts_with(&format!("{tag} ")) {
                return match line.split_whitespace().nth(1) {
                    Some("OK") => Ok(()),
                    _ => Err(Error::Server(line.trim_end().to_string())),
                };
            }
        }
    }

    /// Fetch whole messages by uid range.
    ///
    /// Returns `(uid, flags, rfc822 bytes)`. Literals are read by the
    /// length the server announced rather than by scanning for a
    /// terminator — a message body contains every byte sequence a
    /// terminator could be made of.
    pub async fn fetch_full(
        &mut self,
        range: &str,
    ) -> Result<Vec<(u32, crate::Fetch, Vec<u8>)>, Error> {
        let tag = self.next_tag();
        self.raw(&format!(
            "{tag} UID FETCH {range} (UID FLAGS BODY.PEEK[])\r\n"
        ))
        .await?;
        let mut out = Vec::new();
        loop {
            let line = self.read_line().await?;
            if line.starts_with(&format!("{tag} ")) {
                return finish(line, out);
            }
            let Some(Untagged::Fetch(f)) = parse_line(line.trim_end()) else {
                continue;
            };
            let Some(len) = f.literal_len else { continue };
            let mut body = vec![0u8; len as usize];
            self.io
                .read_exact(&mut body)
                .await
                .map_err(|e| Error::Io(e.to_string()))?;
            if let Some(uid) = f.uid {
                out.push((uid, f, body));
            }
        }
    }

    /// Fetch identities without bodies.
    ///
    /// The cheap half of a re-alignment: a few hundred bytes per
    /// message where `BODY.PEEK[]` is the whole thing. A ten-year
    /// mailbox answers this in seconds and the full fetch in
    /// gigabytes.
    ///
    /// Lines this cannot read are skipped rather than guessed at. A
    /// wrong Message-ID is worse than a missing one: the missing
    /// message is fetched again, and the wrong one is filed as a
    /// different message or merged into somebody else's thread.
    pub async fn fetch_envelopes(&mut self, range: &str) -> Result<Vec<crate::Envelope>, Error> {
        let tag = self.next_tag();
        self.raw(&format!("{tag} UID FETCH {range} (UID ENVELOPE)\r\n"))
            .await?;
        let mut out = Vec::new();
        loop {
            let line = self.read_line().await?;
            if line.starts_with(&format!("{tag} ")) {
                let rest = line[tag.len() + 1..].trim().to_string();
                return match rest.split(' ').next() {
                    Some("OK") => Ok(out),
                    _ => Err(Error::Server(line.trim().to_string())),
                };
            }
            // An ENVELOPE may be folded across lines by a server that
            // sends literals for a header it cannot represent inline.
            // Those are rare and this does not stitch them; the message
            // is simply fetched in full, which is correct and only
            // costs one message's bytes.
            if let Some(e) = crate::parse_envelope(line.trim_end()) {
                out.push(e);
            }
        }
    }

    async fn command(&mut self, cmd: &str) -> Result<(String, Vec<String>), Error> {
        let tag = self.next_tag();
        self.raw(&format!("{tag} {cmd}\r\n")).await?;
        let mut lines = Vec::new();
        loop {
            let line = self.read_line().await?;
            if line.starts_with(&format!("{tag} ")) {
                let rest = line[tag.len() + 1..].trim().to_string();
                return match rest.split(' ').next() {
                    Some("OK") => Ok((rest, lines)),
                    _ => Err(Error::Server(line.trim().to_string())),
                };
            }
            lines.push(line.trim_end().to_string());
        }
    }

    async fn raw(&mut self, s: &str) -> Result<(), Error> {
        self.io
            .get_mut()
            .write_all(s.as_bytes())
            .await
            .map_err(|e| Error::Io(e.to_string()))?;
        self.io
            .get_mut()
            .flush()
            .await
            .map_err(|e| Error::Io(e.to_string()))
    }

    async fn read_line(&mut self) -> Result<String, Error> {
        let mut line = String::new();
        let n = self
            .io
            .read_line(&mut line)
            .await
            .map_err(|e| Error::Io(e.to_string()))?;
        if n == 0 {
            return Err(Error::Io("the server closed the connection".into()));
        }
        Ok(line)
    }

    fn next_tag(&mut self) -> String {
        self.tag += 1;
        format!("a{:04}", self.tag)
    }
}

fn finish(
    tagged: String,
    out: Vec<(u32, crate::Fetch, Vec<u8>)>,
) -> Result<Vec<(u32, crate::Fetch, Vec<u8>)>, Error> {
    let rest = tagged.split(' ').nth(1).unwrap_or_default();
    match rest {
        "OK" => Ok(out),
        _ => Err(Error::Server(tagged.trim().to_string())),
    }
}

/// An IMAP quoted string.
///
/// Backslash and quote are the only two characters that need escaping
/// (RFC 3501 §4.3), and generated app passwords contain both often
/// enough that not doing it reads as "wrong password".
fn quoted(v: &str) -> String {
    let mut s = String::with_capacity(v.len() + 2);
    s.push('"');
    for c in v.chars() {
        if c == '"' || c == '\\' {
            s.push('\\');
        }
        s.push(c);
    }
    s.push('"');
    s
}

async fn wrap_tls(
    tcp: impl Rw + 'static,
    host: &str,
) -> Result<tokio_rustls::client::TlsStream<impl Rw + 'static>, Error> {
    // The same anchors SMTP verifies against: the platform store *and*
    // the compiled-in Mozilla set. `webpki-roots` alone is a browser
    // programme, and that difference stopped mail on 2026-08-17.
    let config = Arc::new(mailrs_smtp_client::default_pkix_client_config());
    let name = rustls::pki_types::ServerName::try_from(host.to_string())
        .map_err(|e| Error::Tls(e.to_string()))?;
    TlsConnector::from(config)
        .connect(name, tcp)
        .await
        .map_err(|e| Error::Tls(e.to_string()))
}

/// A placeholder while the stream is swapped during `STARTTLS`.
struct NullIo;

impl tokio::io::AsyncRead for NullIo {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        _: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}

impl tokio::io::AsyncWrite for NullIo {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::task::Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}
