//! ManageSieve server (RFC 5804) — script CRUD over :4190 (G5).
//!
//! Scripts live in the network kevy `sieve:<address>` string, the same
//! key the spool drain reads and webapi's admin API writes — one source
//! of truth. This server lets a standard sieve client (sieve-connect,
//! Thunderbird's sieve add-on, …) manage the active script directly.
//!
//! Supported: STARTTLS (RFC 5804 §2.2, when a certificate is configured;
//! no AUTHENTICATE until it is used), AUTHENTICATE PLAIN (argon2 against
//! the kevy account blob, same as IMAP/POP3), LISTSCRIPTS, GETSCRIPT,
//! PUTSCRIPT (compile-validated via mailrs-sieve), SETACTIVE,
//! DELETESCRIPT, CHECKSCRIPT, HAVESPACE, CAPABILITY, NOOP, LOGOUT.
//!
//! Single-script model: mailrs stores exactly one script per user, so a
//! script name is cosmetic — PUTSCRIPT/SETACTIVE on any name writes the
//! one `sieve:<addr>` value; LISTSCRIPTS reports it (active) when set.

use std::sync::Arc;

use mailrs_tls_reload::TlsState;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use crate::FastcoreState;
use auth::handle_authenticate;
use script::{delete_script, load_script, read_script_literal, sanitize, store_script};

mod auth;
mod script;
#[cfg(test)]
mod store_tests;
#[cfg(test)]
mod tls_tests;

/// The single script name we present. Real name is irrelevant (one
/// script per user) but a stable label keeps clients happy.
const SCRIPT_NAME: &str = "mailrs";

/// Spawn the plaintext ManageSieve listener on `MAILRS_MANAGESIEVE_BIND`
/// (default `0.0.0.0:4190`). Set the env to `off` to disable. Without a
/// certificate (dev) STARTTLS is not offered and AUTHENTICATE works in
/// clear.
pub async fn spawn(state: Arc<FastcoreState>, tls: Option<TlsState>) {
    let bind =
        std::env::var("MAILRS_MANAGESIEVE_BIND").unwrap_or_else(|_| "0.0.0.0:4190".to_string());
    if bind.eq_ignore_ascii_case("off") || bind.is_empty() {
        tracing::debug!("MAILRS_MANAGESIEVE_BIND=off — skipping ManageSieve");
        return;
    }
    let listener = match TcpListener::bind(&bind).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(error = %e, %bind, "managesieve: bind failed; disabling");
            return;
        }
    };
    if tls.is_none() {
        tracing::warn!(%bind, "managesieve: no certificate — no STARTTLS, passwords are accepted in clear");
    }
    tracing::info!(%bind, starttls = tls.is_some(), "managesieve: listening");
    serve_plain(listener, state, tls).await;
}

async fn serve_plain(listener: TcpListener, state: Arc<FastcoreState>, tls: Option<TlsState>) {
    loop {
        let (sock, peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(error = %e, "managesieve: accept error");
                continue;
            }
        };
        let state = state.clone();
        let tls = tls.clone();
        tokio::spawn(async move {
            if let Err(e) = session(state, sock, tls).await {
                tracing::debug!(%peer, error = %e, "managesieve: session ended");
            }
        });
    }
}

/// Capability list per RFC 5804 §1.7, sent at connect, on CAPABILITY and
/// after STARTTLS. Before TLS no mechanism is on offer, so SASL is empty.
fn greeting(starttls: bool) -> String {
    let mut s = String::new();
    s.push_str("\"IMPLEMENTATION\" \"mailrs\"\r\n");
    s.push_str("\"SIEVE\" \"fileinto reject vacation envelope\"\r\n");
    if starttls {
        s.push_str("\"SASL\" \"\"\r\n\"STARTTLS\"\r\n");
    } else {
        s.push_str("\"SASL\" \"PLAIN\"\r\n");
    }
    s.push_str("\"VERSION\" \"1.0\"\r\n");
    s.push_str("OK\r\n");
    s
}

/// `tls`: STARTTLS is on offer. After it the capabilities are sent again
/// on the TLS stream, and whatever was buffered in clear is dropped.
async fn session<S>(
    state: Arc<FastcoreState>,
    sock: S,
    tls: Option<TlsState>,
) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (rx, mut tx) = tokio::io::split(sock);
    let mut reader = BufReader::new(rx);
    tx.write_all(greeting(tls.is_some()).as_bytes()).await?;
    let Some(tls) = tls else {
        return serve(&state, &mut reader, &mut tx, false).await.map(|_| ());
    };
    if !serve(&state, &mut reader, &mut tx, true).await? {
        return Ok(());
    }
    let sock = reader.into_inner().unsplit(tx);
    let stream = tls.acceptor().accept(sock).await?;
    let (rx, mut tx) = tokio::io::split(stream);
    tx.write_all(greeting(false).as_bytes()).await?;
    serve(&state, &mut BufReader::new(rx), &mut tx, false)
        .await
        .map(|_| ())
}

/// Runs commands until LOGOUT or EOF (false), or until the client is told
/// to begin TLS (true, only when `starttls`).
async fn serve<R, W>(
    state: &Arc<FastcoreState>,
    reader: &mut R,
    tx: &mut W,
    starttls: bool,
) -> std::io::Result<bool>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut authed_user: Option<String> = None;
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).await? == 0 {
            return Ok(false);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let mut parts = trimmed.splitn(2, ' ');
        let cmd = parts.next().unwrap_or("").to_ascii_uppercase();
        let arg = parts.next().unwrap_or("").trim();

        match cmd.as_str() {
            "CAPABILITY" => tx.write_all(greeting(starttls).as_bytes()).await?,
            "NOOP" => tx.write_all(b"OK \"NOOP completed\"\r\n").await?,
            "LOGOUT" => {
                tx.write_all(b"OK \"Bye\"\r\n").await?;
                return Ok(false);
            }
            "AUTHENTICATE" if starttls => {
                tx.write_all(b"NO (ENCRYPT-NEEDED) \"use STARTTLS first\"\r\n")
                    .await?;
            }
            "STARTTLS" if starttls => {
                tx.write_all(b"OK \"Begin TLS negotiation now\"\r\n")
                    .await?;
                return Ok(true);
            }
            "AUTHENTICATE" => match handle_authenticate(state, arg, reader, tx).await? {
                Some(user) => {
                    authed_user = Some(user);
                    tx.write_all(b"OK \"Authenticated\"\r\n").await?;
                }
                None => tx.write_all(b"NO \"Authentication failed\"\r\n").await?,
            },
            "HAVESPACE" => {
                // we impose no per-script size limit — always yes
                tx.write_all(b"OK \"Within limits\"\r\n").await?;
            }
            "CHECKSCRIPT" => {
                let (_name, body) = read_script_literal(arg, reader).await?;
                match mailrs_sieve::compile_sieve(&body) {
                    Ok(_) => tx.write_all(b"OK \"Script is valid\"\r\n").await?,
                    Err(e) => {
                        tx.write_all(format!("NO \"{}\"\r\n", sanitize(&e)).as_bytes())
                            .await?
                    }
                }
            }
            "PUTSCRIPT" => {
                let Some(user) = &authed_user else {
                    tx.write_all(b"NO \"Authenticate first\"\r\n").await?;
                    continue;
                };
                let (_name, body) = read_script_literal(arg, reader).await?;
                match mailrs_sieve::compile_sieve(&body) {
                    Ok(_) => match store_script(state, user, &body) {
                        Ok(()) => tx.write_all(b"OK \"Script stored\"\r\n").await?,
                        Err(e) => tx.write_all(store_error(&e).as_bytes()).await?,
                    },
                    Err(e) => {
                        tx.write_all(format!("NO \"{}\"\r\n", sanitize(&e)).as_bytes())
                            .await?
                    }
                }
            }
            "GETSCRIPT" => {
                let Some(user) = &authed_user else {
                    tx.write_all(b"NO \"Authenticate first\"\r\n").await?;
                    continue;
                };
                match load_script(state, user) {
                    Ok(Some(body)) => {
                        tx.write_all(format!("{{{}}}\r\n", body.len()).as_bytes())
                            .await?;
                        tx.write_all(body.as_bytes()).await?;
                        tx.write_all(b"\r\nOK\r\n").await?;
                    }
                    Ok(None) => tx.write_all(b"NO \"No such script\"\r\n").await?,
                    Err(e) => tx.write_all(store_error(&e).as_bytes()).await?,
                }
            }
            "LISTSCRIPTS" => {
                let Some(user) = &authed_user else {
                    tx.write_all(b"NO \"Authenticate first\"\r\n").await?;
                    continue;
                };
                match load_script(state, user) {
                    Ok(found) => {
                        if found.is_some() {
                            tx.write_all(format!("\"{SCRIPT_NAME}\" ACTIVE\r\n").as_bytes())
                                .await?;
                        }
                        tx.write_all(b"OK \"Listscripts completed\"\r\n").await?;
                    }
                    Err(e) => tx.write_all(store_error(&e).as_bytes()).await?,
                }
            }
            "SETACTIVE" => {
                // one-script model: activating the only script is a
                // no-op success; SETACTIVE "" (deactivate) clears it
                let Some(user) = &authed_user else {
                    tx.write_all(b"NO \"Authenticate first\"\r\n").await?;
                    continue;
                };
                let name = unquote(arg);
                let cleared = match name.is_empty() {
                    true => delete_script(state, user),
                    false => Ok(()),
                };
                match cleared {
                    Ok(()) => tx.write_all(b"OK \"Setactive completed\"\r\n").await?,
                    Err(e) => tx.write_all(store_error(&e).as_bytes()).await?,
                }
            }
            "DELETESCRIPT" => {
                let Some(user) = &authed_user else {
                    tx.write_all(b"NO \"Authenticate first\"\r\n").await?;
                    continue;
                };
                match delete_script(state, user) {
                    Ok(()) => tx.write_all(b"OK \"Deleted\"\r\n").await?,
                    Err(e) => tx.write_all(store_error(&e).as_bytes()).await?,
                }
            }
            "STARTTLS" => {
                tx.write_all(b"NO \"STARTTLS not available\"\r\n").await?;
            }
            _ => {
                tx.write_all(b"NO \"Unknown command\"\r\n").await?;
            }
        }
    }
}

fn store_error(e: &std::io::Error) -> String {
    tracing::warn!(err = %e, "managesieve: script store failed");
    format!("NO (TRYLATER) \"{}\"\r\n", sanitize(&e.to_string()))
}

fn unquote(s: &str) -> &str {
    s.trim().trim_matches('"')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unquote_strips_quotes() {
        assert_eq!(unquote("\"PLAIN\""), "PLAIN");
        assert_eq!(unquote("  bare  "), "bare");
    }

    #[test]
    fn greeting_advertises_plain_and_version() {
        let g = greeting(false);
        assert!(g.contains("\"SASL\" \"PLAIN\""));
        assert!(g.contains("\"VERSION\" \"1.0\""));
        assert!(g.ends_with("OK\r\n"));
    }
}
