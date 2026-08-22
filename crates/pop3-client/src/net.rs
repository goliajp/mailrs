//! The socket half: connect, log in, list, fetch.
//!
//! Behind the `net` feature, so the parsing and the deduplication stay
//! testable without a TLS stack — the same split `mailrs-imap-client`
//! uses, and for the same reason.

use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use crate::{Line, Uid, is_authentication_failure, parse_line, parse_uidl};

/// How a connection is protected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tls {
    /// TLS from the first byte — port 995.
    Implicit,
    /// Plain, upgraded with `STLS` (RFC 2595).
    StartTls,
    /// None.
    None,
}

/// What went wrong, in words that reach the row and then the screen.
#[derive(Debug)]
pub enum Error {
    /// Could not reach the server.
    Connect(String),
    /// TLS did not come up.
    Tls(String),
    /// The credential was refused. **Not retried on a timer.**
    Auth(String),
    /// The server said no to something else.
    Server(String),
    /// The connection broke mid-conversation.
    Io(String),
    /// The server has no `UIDL`, so its mail cannot be deduplicated.
    ///
    /// Named rather than lumped in with `Server`, because it is not a
    /// transient failure and not a wrong password: it is a property of
    /// the server, it will not change, and the person needs to be told
    /// at set-up rather than have their mailbox re-downloaded forever.
    NoUidl,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connect(m) => write!(f, "cannot reach the server: {m}"),
            Self::Tls(m) => write!(f, "TLS failed: {m}"),
            Self::Auth(m) => write!(f, "the password was refused: {m}"),
            Self::Server(m) => write!(f, "the server refused: {m}"),
            Self::Io(m) => write!(f, "the connection broke: {m}"),
            Self::NoUidl => write!(
                f,
                "this server does not support UIDL, so its messages cannot be \
                 told apart between syncs — every sync would download the \
                 mailbox again"
            ),
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    /// Whether waiting could ever help.
    pub fn is_permanent(&self) -> bool {
        matches!(self, Self::Auth(_) | Self::NoUidl)
    }
}

/// Either half of the connection, before and after `STLS`.
pub trait Rw: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Rw for T {}

/// One POP3 conversation.
pub struct Session {
    io: BufReader<Box<dyn Rw>>,
}

impl Session {
    /// Open a connection and read the greeting.
    pub async fn connect(host: &str, port: u16, tls: Tls) -> Result<Self, Error> {
        let tcp = TcpStream::connect((host, port))
            .await
            .map_err(|e| Error::Connect(e.to_string()))?;
        let stream: Box<dyn Rw> = match tls {
            Tls::Implicit => Box::new(wrap_tls(tcp, host).await?),
            Tls::None | Tls::StartTls => Box::new(tcp),
        };
        let mut s = Self {
            io: BufReader::new(stream),
        };
        match parse_line(&s.read_line().await?) {
            Line::Ok(_) => {}
            other => return Err(Error::Server(format!("{other:?}"))),
        }
        Ok(s)
    }

    /// `USER` / `PASS`.
    ///
    /// A refused login here is permanent: waiting cannot fix a password
    /// that changed, and some providers count the attempts.
    pub async fn login(&mut self, user: &str, secret: &str) -> Result<(), Error> {
        self.command(&format!("USER {user}")).await?;
        match self.command(&format!("PASS {secret}")).await {
            Ok(_) => Ok(()),
            Err(Error::Server(m)) if is_authentication_failure(&m) => Err(Error::Auth(m)),
            // A bare `-ERR` from PASS is a refused credential too: not
            // every server explains itself, and treating those as
            // retryable hammers the provider with the wrong password.
            Err(Error::Server(m)) => Err(Error::Auth(m)),
            Err(e) => Err(e),
        }
    }

    /// `UIDL` — every message's durable identity.
    pub async fn uidl(&mut self) -> Result<Vec<Uid>, Error> {
        match self.command("UIDL").await {
            Ok(_) => {}
            // Only when the server says it does not know the command.
            // A locked mailbox answers `-ERR` too, and marking the
            // account permanently broken over a lock that clears in a
            // minute is something only a person can undo.
            Err(Error::Server(m)) if crate::no_uidl(&format!("-ERR {m}")) => {
                return Err(Error::NoUidl);
            }
            Err(e) => return Err(e),
        }
        let lines = self.read_multiline().await?;
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        Ok(parse_uidl(&refs))
    }

    /// `RETR` — one whole message.
    pub async fn retr(&mut self, number: u32) -> Result<Vec<u8>, Error> {
        self.command(&format!("RETR {number}")).await?;
        let lines = self.read_multiline().await?;
        Ok(lines.join("\r\n").into_bytes())
    }

    /// `QUIT`, which is also what commits any `DELE`.
    pub async fn quit(&mut self) -> Result<(), Error> {
        let _ = self.command("QUIT").await;
        Ok(())
    }

    async fn command(&mut self, cmd: &str) -> Result<String, Error> {
        self.io
            .get_mut()
            .write_all(format!("{cmd}\r\n").as_bytes())
            .await
            .map_err(|e| Error::Io(e.to_string()))?;
        self.io
            .get_mut()
            .flush()
            .await
            .map_err(|e| Error::Io(e.to_string()))?;
        match parse_line(&self.read_line().await?) {
            Line::Ok(rest) => Ok(rest),
            Line::Err(rest) => Err(Error::Server(rest)),
            Line::Data(d) => Err(Error::Server(d)),
        }
    }

    /// Everything up to the lone `.`, with dot-stuffing undone.
    ///
    /// A body line that begins with `.` arrives doubled (RFC 1939
    /// §3), and a client that does not undo it corrupts every message
    /// containing such a line — quoted mail and `..` in code both do.
    async fn read_multiline(&mut self) -> Result<Vec<String>, Error> {
        let mut out = Vec::new();
        loop {
            let line = self.read_line().await?;
            let line = line.trim_end_matches(['\r', '\n']);
            if line == "." {
                return Ok(out);
            }
            out.push(match line.strip_prefix('.') {
                Some(rest) => rest.to_string(),
                None => line.to_string(),
            });
        }
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
}

async fn wrap_tls(
    tcp: impl Rw + 'static,
    host: &str,
) -> Result<tokio_rustls::client::TlsStream<impl Rw + 'static>, Error> {
    // The same anchors SMTP and IMAP verify against: the platform store
    // *and* the compiled-in Mozilla set.
    let config = Arc::new(mailrs_smtp_client::default_pkix_client_config());
    let name = rustls::pki_types::ServerName::try_from(host.to_string())
        .map_err(|e| Error::Tls(e.to_string()))?;
    TlsConnector::from(config)
        .connect(name, tcp)
        .await
        .map_err(|e| Error::Tls(e.to_string()))
}
