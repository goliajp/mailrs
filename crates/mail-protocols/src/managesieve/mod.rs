//! ManageSieve (RFC 5804) — script management on :4190.
//!
//! Supported: STARTTLS (RFC 5804 §2.2, when a certificate is configured;
//! no AUTHENTICATE until it is used), AUTHENTICATE PLAIN, LISTSCRIPTS,
//! GETSCRIPT, PUTSCRIPT (compile-validated via mailrs-sieve), SETACTIVE,
//! DELETESCRIPT, CHECKSCRIPT, HAVESPACE, CAPABILITY, NOOP, LOGOUT.
//!
//! Single-script model: a user has exactly one script, so a script name
//! is cosmetic — PUTSCRIPT/SETACTIVE on any name writes the one script;
//! LISTSCRIPTS reports it (active) when set.

mod auth;
mod script;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use mailrs_tls_reload::TlsState;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use crate::backend::{Account, Directory, Error, Protocol};
use crate::listen;
use auth::plain_credentials;
use script::{read_script_literal, sanitize};

/// The single script name presented. A stable label keeps clients happy.
const SCRIPT_NAME: &str = "mailrs";

/// Plaintext ManageSieve on `MAILRS_MANAGESIEVE_BIND` (default
/// `0.0.0.0:4190`). Without a certificate (dev) STARTTLS is not offered
/// and AUTHENTICATE works in clear.
pub async fn spawn<D: Directory>(dir: Arc<D>, tls: Option<TlsState>) {
    let bound = listen::bind(listen::MANAGESIEVE).await;
    let Some(listener) = bound else {
        return;
    };
    if tls.is_none() {
        tracing::warn!(
            "managesieve: no certificate — no STARTTLS, passwords are accepted in clear"
        );
    }
    tracing::info!(starttls = tls.is_some(), "managesieve: listening");
    serve_plain(listener, dir, tls).await;
}

pub async fn serve_plain<D: Directory>(listener: TcpListener, dir: Arc<D>, tls: Option<TlsState>) {
    listen::accept(listener, "managesieve", move |sock, peer| {
        let dir = dir.clone();
        let tls = tls.clone();
        async move {
            if let Err(e) = session(dir, sock, tls).await {
                tracing::debug!(%peer, error = %e, "managesieve: session ended");
            }
        }
    })
    .await;
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

/// One connection. `tls`: STARTTLS is on offer. After it the capabilities
/// are sent again on the TLS stream, and whatever was buffered in clear is
/// dropped.
pub async fn session<D, S>(dir: Arc<D>, sock: S, tls: Option<TlsState>) -> std::io::Result<()>
where
    D: Directory,
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (rx, mut tx) = tokio::io::split(sock);
    let mut reader = BufReader::new(rx);
    tx.write_all(greeting(tls.is_some()).as_bytes()).await?;
    let Some(tls) = tls else {
        return serve(&*dir, &mut reader, &mut tx, false).await.map(drop);
    };
    if !serve(&*dir, &mut reader, &mut tx, true).await? {
        return Ok(());
    }
    let sock = reader.into_inner().unsplit(tx);
    let stream = tls.acceptor().accept(sock).await?;
    let (rx, mut tx) = tokio::io::split(stream);
    tx.write_all(greeting(false).as_bytes()).await?;
    serve(&*dir, &mut BufReader::new(rx), &mut tx, false)
        .await
        .map(drop)
}

/// Runs commands until LOGOUT or EOF (false), or until the client is told
/// to begin TLS (true, only when `starttls`).
async fn serve<D, R, W>(
    dir: &D,
    reader: &mut R,
    tx: &mut W,
    starttls: bool,
) -> std::io::Result<bool>
where
    D: Directory,
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut account: Option<D::Account> = None;
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
            "STARTTLS" => tx.write_all(b"NO \"STARTTLS not available\"\r\n").await?,
            "AUTHENTICATE" => {
                if let Some(signed_in) = authenticate(dir, arg, reader, tx).await? {
                    account = Some(signed_in);
                }
            }
            "HAVESPACE" => tx.write_all(havespace(arg).as_bytes()).await?,
            "CHECKSCRIPT" => {
                let (_name, body) = read_script_literal(arg, reader).await?;
                let reply = match mailrs_sieve::compile_sieve(&body) {
                    Ok(_) => "OK \"Script is valid\"\r\n".to_string(),
                    Err(e) => format!("NO \"{}\"\r\n", sanitize(&e)),
                };
                tx.write_all(reply.as_bytes()).await?;
            }
            "PUTSCRIPT" | "GETSCRIPT" | "LISTSCRIPTS" | "SETACTIVE" | "DELETESCRIPT" => {
                match &account {
                    Some(a) => scripts(a, &cmd, arg, reader, tx).await?,
                    None => tx.write_all(b"NO \"Authenticate first\"\r\n").await?,
                }
            }
            _ => tx.write_all(b"NO \"Unknown command\"\r\n").await?,
        }
    }
}

async fn authenticate<D, R, W>(
    dir: &D,
    arg: &str,
    reader: &mut R,
    tx: &mut W,
) -> std::io::Result<Option<D::Account>>
where
    D: Directory,
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let signed_in = match plain_credentials(arg, reader, tx).await? {
        Some((user, pass)) => dir.login(&user, &pass, Protocol::ManageSieve).await,
        None => Ok(None),
    };
    match signed_in {
        Ok(Some(a)) => {
            tx.write_all(b"OK \"Authenticated\"\r\n").await?;
            Ok(Some(a))
        }
        Ok(None) => {
            tx.write_all(b"NO \"Authentication failed\"\r\n").await?;
            Ok(None)
        }
        Err(e) => {
            tx.write_all(store_error(&e).as_bytes()).await?;
            Ok(None)
        }
    }
}

/// The commands on the signed-in user's script.
async fn scripts<A, R, W>(
    account: &A,
    cmd: &str,
    arg: &str,
    reader: &mut R,
    tx: &mut W,
) -> std::io::Result<()>
where
    A: Account,
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let reply = match cmd {
        "PUTSCRIPT" => {
            let (_name, body) = read_script_literal(arg, reader).await?;
            match mailrs_sieve::compile_sieve(&body) {
                Ok(_) => answer(
                    account.put_sieve_script(&body).await,
                    "OK \"Script stored\"\r\n",
                ),
                Err(e) => format!("NO \"{}\"\r\n", sanitize(&e)),
            }
        }
        "GETSCRIPT" => match account.sieve_script().await {
            Ok(Some(body)) => format!("{{{}}}\r\n{body}\r\nOK\r\n", body.len()),
            Ok(None) => "NO \"No such script\"\r\n".to_string(),
            Err(e) => store_error(&e),
        },
        "LISTSCRIPTS" => match account.sieve_script().await {
            Ok(Some(_)) => format!("\"{SCRIPT_NAME}\" ACTIVE\r\nOK \"Listscripts completed\"\r\n"),
            Ok(None) => "OK \"Listscripts completed\"\r\n".to_string(),
            Err(e) => store_error(&e),
        },
        // one-script model: activating the only script is a no-op
        // success; SETACTIVE "" (deactivate) clears it
        "SETACTIVE" => match unquote(arg).is_empty() {
            true => answer(
                account.delete_sieve_script().await,
                "OK \"Setactive completed\"\r\n",
            ),
            false => "OK \"Setactive completed\"\r\n".to_string(),
        },
        _ => answer(account.delete_sieve_script().await, "OK \"Deleted\"\r\n"),
    };
    tx.write_all(reply.as_bytes()).await
}

/// `HAVESPACE "name" size`: the one limit is the literal a script can
/// arrive in.
fn havespace(arg: &str) -> String {
    let size = arg.rsplit(' ').next().and_then(|n| n.parse::<usize>().ok());
    match size {
        Some(n) if n > script::MAX_LITERAL => {
            "NO (QUOTA/MAXSIZE) \"Script too large\"\r\n".to_string()
        }
        _ => "OK \"Within limits\"\r\n".to_string(),
    }
}

fn answer(done: Result<(), Error>, ok: &str) -> String {
    match done {
        Ok(()) => ok.to_string(),
        Err(e) => store_error(&e),
    }
}

fn store_error(e: &Error) -> String {
    tracing::warn!(err = %e, "managesieve: script store failed");
    format!("NO (TRYLATER) \"{}\"\r\n", sanitize(&e.to_string()))
}

fn unquote(s: &str) -> &str {
    s.trim().trim_matches('"')
}
