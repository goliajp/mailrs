//! POP3 (RFC 1939).
//!
//! A single-INBOX view: messages are numbered 1..N in uid order and RETR
//! sends the raw bytes. TOP (RFC 1939 §7) trims to N body lines; DELE
//! marks for delete-at-QUIT. On a plaintext port with a certificate, STLS
//! (RFC 2595) is offered and no credentials are accepted until it is used.

mod maildrop;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use mailrs_tls_reload::TlsState;
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};
use tokio::net::TcpListener;

use crate::backend::{Directory, Protocol};
use crate::listen;
use maildrop::Maildrop;

/// Plaintext POP3 on `MAILRS_POP3_BIND` (default `0.0.0.0:110`). Without
/// a certificate (dev) STLS is not offered and USER / PASS work in clear.
pub async fn spawn<D: Directory>(dir: Arc<D>, tls: Option<TlsState>) {
    let Some(listener) = listen::bind("MAILRS_POP3_BIND", "0.0.0.0:110", "pop3").await else {
        return;
    };
    if tls.is_none() {
        tracing::warn!("pop3: no certificate — no STLS, passwords are accepted in clear");
    }
    tracing::info!(stls = tls.is_some(), "pop3: listening");
    serve_plain(listener, dir, tls).await;
}

pub async fn serve_plain<D: Directory>(listener: TcpListener, dir: Arc<D>, tls: Option<TlsState>) {
    listen::accept(listener, "pop3", move |sock, peer| {
        let dir = dir.clone();
        let tls = tls.clone();
        async move {
            if let Err(e) = session(dir, sock, tls).await {
                tracing::debug!(%peer, error = %e, "pop3: session ended");
            }
        }
    })
    .await;
}

/// Implicit-TLS POP3S on `MAILRS_POP3S_BIND` (default `0.0.0.0:995`).
pub async fn spawn_tls<D: Directory>(dir: Arc<D>, tls: TlsState) {
    let Some(listener) = listen::bind("MAILRS_POP3S_BIND", "0.0.0.0:995", "pop3s").await else {
        return;
    };
    tracing::info!("pop3s: listening (implicit TLS)");
    serve_tls(listener, dir, tls).await;
}

pub async fn serve_tls<D: Directory>(listener: TcpListener, dir: Arc<D>, tls: TlsState) {
    listen::accept_tls(listener, "pop3s", tls, move |stream| {
        let dir = dir.clone();
        async move {
            if let Err(e) = session(dir, stream, None).await {
                tracing::debug!(error = %e, "pop3s: session ended");
            }
        }
    })
    .await;
}

/// One connection. `tls`: STLS is on offer. After it the session starts
/// over on the TLS stream with no greeting, and whatever was buffered in
/// clear is dropped.
pub async fn session<D, S>(dir: Arc<D>, sock: S, tls: Option<TlsState>) -> std::io::Result<()>
where
    D: Directory,
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let (rx, mut tx) = tokio::io::split(sock);
    let mut reader = BufReader::new(rx);
    tx.write_all(b"+OK mailrs POP3 ready\r\n").await?;
    let Some(tls) = tls else {
        return serve(&*dir, &mut reader, &mut tx, false).await.map(drop);
    };
    if !serve(&*dir, &mut reader, &mut tx, true).await? {
        return Ok(());
    }
    let sock = reader.into_inner().unsplit(tx);
    let stream = tls.acceptor().accept(sock).await?;
    let (rx, mut tx) = tokio::io::split(stream);
    serve(&*dir, &mut BufReader::new(rx), &mut tx, false)
        .await
        .map(drop)
}

/// The longest command line read, CRLF included, before sign-in as after
/// it. RFC 2449 §4 caps a command at 255 octets; this leaves room for a
/// long password.
const MAX_LINE: u64 = 8 * 1024;

/// Runs commands until QUIT or EOF (false), or until the client is told
/// to begin TLS (true, only when `stls`).
async fn serve<D, R, W>(dir: &D, reader: &mut R, tx: &mut W, stls: bool) -> std::io::Result<bool>
where
    D: Directory,
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut user: Option<String> = None;
    let mut maildrop: Option<Maildrop<D::Account>> = None;
    loop {
        let mut line = String::new();
        let n = (&mut *reader).take(MAX_LINE).read_line(&mut line).await?;
        if n == 0 {
            return Ok(false);
        }
        if !line.ends_with('\n') && n as u64 == MAX_LINE {
            tx.write_all(b"-ERR line too long, closing\r\n").await?;
            return Ok(false);
        }
        let cleaned = line.trim_end_matches(['\r', '\n']);
        let mut parts = cleaned.splitn(2, ' ');
        let verb = parts.next().unwrap_or("").to_uppercase();
        let arg = parts.next().unwrap_or("").trim();

        if let Some(md) = maildrop.as_mut()
            && md.handle(&verb, arg, tx).await?
        {
            continue;
        }
        match verb.as_str() {
            "USER" | "PASS" | "APOP" | "AUTH" if stls => {
                tx.write_all(b"-ERR [AUTH] use STLS first\r\n").await?;
            }
            "STLS" if stls => {
                tx.write_all(b"+OK Begin TLS negotiation\r\n").await?;
                return Ok(true);
            }
            "STLS" => tx.write_all(b"-ERR STLS not available\r\n").await?,
            "USER" => {
                user = Some(arg.to_string());
                tx.write_all(b"+OK send PASS\r\n").await?;
            }
            "PASS" => match &user {
                Some(username) => {
                    if let Some(opened) = pass(dir, username, arg, tx).await? {
                        maildrop = Some(opened);
                    }
                }
                None => tx.write_all(b"-ERR USER first\r\n").await?,
            },
            "STAT" | "LIST" | "UIDL" | "RETR" | "TOP" | "DELE" => {
                tx.write_all(b"-ERR not authenticated\r\n").await?;
            }
            "RSET" => {
                if let Some(md) = maildrop.as_mut() {
                    md.reset();
                }
                tx.write_all(b"+OK maildrop reset\r\n").await?;
            }
            "NOOP" => tx.write_all(b"+OK\r\n").await?,
            "CAPA" => tx.write_all(capa(stls).as_bytes()).await?,
            "QUIT" => {
                quit(maildrop.as_ref(), tx).await?;
                return Ok(false);
            }
            _ => tx.write_all(b"-ERR unknown command\r\n").await?,
        }
    }
}

/// PASS: sign in and open the maildrop, answering either way.
async fn pass<D, W>(
    dir: &D,
    username: &str,
    password: &str,
    tx: &mut W,
) -> std::io::Result<Option<Maildrop<D::Account>>>
where
    D: Directory,
    W: AsyncWrite + Unpin,
{
    let account = match dir.login(username, password, Protocol::Pop3).await {
        Ok(Some(a)) => a,
        Ok(None) => {
            tx.write_all(b"-ERR invalid credentials\r\n").await?;
            return Ok(None);
        }
        Err(e) => return temp_failure(&e, tx).await.map(|()| None),
    };
    match Maildrop::open(account).await {
        Ok(opened) => {
            tx.write_all(format!("+OK {} messages\r\n", opened.len()).as_bytes())
                .await?;
            Ok(Some(opened))
        }
        Err(e) => temp_failure(&e, tx).await.map(|()| None),
    }
}

async fn temp_failure<W: AsyncWrite + Unpin>(
    e: &crate::backend::Error,
    tx: &mut W,
) -> std::io::Result<()> {
    tracing::warn!(err = %e, "pop3: maildrop unavailable");
    let text = e.to_string().replace(['\r', '\n'], " ");
    tx.write_all(format!("-ERR [SYS/TEMP] {text}\r\n").as_bytes())
        .await
}

async fn quit<A, W>(maildrop: Option<&Maildrop<A>>, tx: &mut W) -> std::io::Result<()>
where
    A: crate::backend::Account,
    W: AsyncWrite + Unpin,
{
    let committed = match maildrop {
        Some(md) => md.commit().await,
        None => Ok(()),
    };
    match committed {
        Ok(()) => tx.write_all(b"+OK bye\r\n").await,
        Err(e) => {
            tracing::warn!(err = %e, "pop3: QUIT could not remove a message");
            tx.write_all(b"-ERR some deleted messages not removed\r\n")
                .await
        }
    }
}

/// RFC 2449. Before STLS, USER is withheld since it would be refused.
fn capa(stls: bool) -> String {
    let login = if stls { "STLS" } else { "USER" };
    format!(
        "+OK Capability list follows\r\n{login}\r\nUIDL\r\nTOP\r\nRESP-CODES\r\nAUTH-RESP-CODE\r\n.\r\n"
    )
}
