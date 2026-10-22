//! Fastcore-native POP3 server (RFC 1939).
//!
//! Simpler than IMAP by design: a POP3 session is essentially a
//! single-INBOX read-only view where messages are numbered 1..N in
//! scan order and RETR streams the raw file bytes. Optional TOP
//! (RFC 1939 §7) trims to N body lines; DELE marks for delete-at-QUIT.
//!
//! Auth uses the same kevy account store as IMAP. On a plaintext port
//! with a certificate, STLS (RFC 2595) is offered and no credentials are
//! accepted until it is used.

mod maildrop;
#[cfg(test)]
mod tls_tests;

use std::sync::Arc;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use mailrs_tls_reload::TlsState;

use crate::FastcoreState;
use crate::imap::backend;
use maildrop::Maildrop;

/// Spawn plaintext POP3 listener on `MAILRS_POP3_BIND` (default
/// `0.0.0.0:110`). Set the env to `off` to disable. Without a
/// certificate (dev) STLS is not offered and USER / PASS work in clear.
pub async fn spawn(state: Arc<FastcoreState>, tls: Option<TlsState>) {
    let bind = std::env::var("MAILRS_POP3_BIND").unwrap_or_else(|_| "0.0.0.0:110".to_string());
    if bind.eq_ignore_ascii_case("off") || bind.is_empty() {
        tracing::debug!("MAILRS_POP3_BIND=off — skipping POP3 listener");
        return;
    }
    let listener = match TcpListener::bind(&bind).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(error = %e, %bind, "pop3: bind failed; disabling POP3");
            return;
        }
    };
    if tls.is_none() {
        tracing::warn!(%bind, "pop3: no certificate — no STLS, passwords are accepted in clear");
    }
    tracing::info!(%bind, stls = tls.is_some(), "pop3: listening");
    serve_plain(listener, state, tls).await;
}

async fn serve_plain(listener: TcpListener, state: Arc<FastcoreState>, tls: Option<TlsState>) {
    loop {
        let (sock, peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(error = %e, "pop3: accept error");
                continue;
            }
        };
        let state = state.clone();
        let tls = tls.clone();
        tokio::spawn(async move {
            tracing::debug!(%peer, "pop3: connection open");
            if let Err(e) = session(state, sock, tls).await {
                tracing::debug!(%peer, error = %e, "pop3: session ended");
            }
        });
    }
}

/// Spawn implicit-TLS POP3S listener on `MAILRS_POP3S_BIND`
/// (default `0.0.0.0:995`), sharing IMAPS's certificate state.
pub async fn spawn_tls(state: Arc<FastcoreState>, tls: TlsState) {
    let bind = std::env::var("MAILRS_POP3S_BIND").unwrap_or_else(|_| "0.0.0.0:995".to_string());
    if bind.eq_ignore_ascii_case("off") || bind.is_empty() {
        tracing::debug!("MAILRS_POP3S_BIND=off — skipping POP3S listener");
        return;
    }
    let listener = match TcpListener::bind(&bind).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(error = %e, %bind, "pop3s: bind failed");
            return;
        }
    };
    tracing::info!(%bind, "pop3s: listening (implicit TLS)");
    serve_tls(listener, state, tls).await;
}

async fn serve_tls(listener: TcpListener, state: Arc<FastcoreState>, tls: TlsState) {
    loop {
        let (sock, peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(error = %e, "pop3s: accept error");
                continue;
            }
        };
        let state = state.clone();
        let acceptor = tls.acceptor();
        tokio::spawn(async move {
            tracing::debug!(%peer, "pop3s: connection open");
            match acceptor.accept(sock).await {
                Ok(tls_sock) => {
                    if let Err(e) = session(state, tls_sock, None).await {
                        tracing::debug!(%peer, error = %e, "pop3s: session ended");
                    }
                }
                Err(e) => tracing::warn!(%peer, error = %e, "pop3s: handshake failed"),
            }
        });
    }
}

/// `tls`: STLS is on offer. After it the session starts over on the TLS
/// stream with no greeting, and whatever was buffered in clear is dropped.
async fn session<S>(
    state: Arc<FastcoreState>,
    sock: S,
    tls: Option<TlsState>,
) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let (rx, mut tx) = tokio::io::split(sock);
    let mut reader = BufReader::new(rx);
    tx.write_all(b"+OK mailrs POP3 ready\r\n").await?;
    let Some(tls) = tls else {
        return serve(&state, &mut reader, &mut tx, false).await.map(drop);
    };
    if !serve(&state, &mut reader, &mut tx, true).await? {
        return Ok(());
    }
    let sock = reader.into_inner().unsplit(tx);
    let stream = tls.acceptor().accept(sock).await?;
    let (rx, mut tx) = tokio::io::split(stream);
    serve(&state, &mut BufReader::new(rx), &mut tx, false)
        .await
        .map(drop)
}

/// Runs commands until QUIT or EOF (false), or until the client is told
/// to begin TLS (true, only when `stls`).
async fn serve<R, W>(
    state: &Arc<FastcoreState>,
    reader: &mut R,
    tx: &mut W,
    stls: bool,
) -> std::io::Result<bool>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut user: Option<String> = None;
    let mut maildrop: Option<Maildrop> = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await? == 0 {
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
            "PASS" => {
                let Some(username) = user.clone() else {
                    tx.write_all(b"-ERR USER first\r\n").await?;
                    continue;
                };
                if backend::verify_password(state, &username, arg) {
                    let opened = Maildrop::open(state, &username);
                    tx.write_all(format!("+OK {} messages\r\n", opened.len()).as_bytes())
                        .await?;
                    maildrop = Some(opened);
                } else {
                    tx.write_all(b"-ERR invalid credentials\r\n").await?;
                }
            }
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
                let committed = maildrop.as_ref().map_or(Ok(()), Maildrop::commit);
                match committed {
                    Ok(()) => tx.write_all(b"+OK bye\r\n").await?,
                    Err(e) => {
                        tracing::warn!(err = %e, "pop3: QUIT could not remove a message");
                        tx.write_all(b"-ERR some deleted messages not removed\r\n")
                            .await?
                    }
                }
                return Ok(false);
            }
            _ => tx.write_all(b"-ERR unknown command\r\n").await?,
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
