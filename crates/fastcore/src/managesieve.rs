//! ManageSieve server (RFC 5804) — script CRUD over :4190 (G5).
//!
//! Scripts live in the network kevy `sieve:<address>` string, the same
//! key the spool drain reads and webapi's admin API writes — one source
//! of truth. This server lets a standard sieve client (sieve-connect,
//! Thunderbird's sieve add-on, …) manage the active script directly.
//!
//! Supported: STARTTLS advertise, AUTHENTICATE PLAIN (argon2 against the
//! kevy account blob, same as IMAP/POP3), LISTSCRIPTS, GETSCRIPT,
//! PUTSCRIPT (compile-validated via mailrs-sieve), SETACTIVE,
//! DELETESCRIPT, CHECKSCRIPT, HAVESPACE, CAPABILITY, NOOP, LOGOUT.
//!
//! Single-script model: mailrs stores exactly one script per user, so a
//! script name is cosmetic — PUTSCRIPT/SETACTIVE on any name writes the
//! one `sieve:<addr>` value; LISTSCRIPTS reports it (active) when set.

use std::sync::Arc;

use kevy_client::Connection;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use crate::FastcoreState;

/// The single script name we present. Real name is irrelevant (one
/// script per user) but a stable label keeps clients happy.
const SCRIPT_NAME: &str = "mailrs";

/// Spawn the plaintext ManageSieve listener on `MAILRS_MANAGESIEVE_BIND`
/// (default `0.0.0.0:4190`). Set the env to `off` to disable.
pub async fn spawn(state: Arc<FastcoreState>) {
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
    tracing::info!(%bind, "managesieve: listening");
    loop {
        let (sock, peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(error = %e, "managesieve: accept error");
                continue;
            }
        };
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = session(state, sock).await {
                tracing::debug!(%peer, error = %e, "managesieve: session ended");
            }
        });
    }
}

fn greeting() -> String {
    // capability list per RFC 5804 §1.7 — advertised at connect + after
    // any state change (we send it once at greeting + after AUTH).
    let mut s = String::new();
    s.push_str("\"IMPLEMENTATION\" \"mailrs\"\r\n");
    s.push_str("\"SIEVE\" \"fileinto reject vacation envelope\"\r\n");
    s.push_str("\"SASL\" \"PLAIN\"\r\n");
    s.push_str("\"VERSION\" \"1.0\"\r\n");
    s.push_str("OK\r\n");
    s
}

async fn session<S>(state: Arc<FastcoreState>, sock: S) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (rx, mut tx) = tokio::io::split(sock);
    let mut reader = BufReader::new(rx);
    tx.write_all(greeting().as_bytes()).await?;

    let mut authed_user: Option<String> = None;
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).await? == 0 {
            return Ok(());
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let mut parts = trimmed.splitn(2, ' ');
        let cmd = parts.next().unwrap_or("").to_ascii_uppercase();
        let arg = parts.next().unwrap_or("").trim();

        match cmd.as_str() {
            "CAPABILITY" => tx.write_all(greeting().as_bytes()).await?,
            "NOOP" => tx.write_all(b"OK \"NOOP completed\"\r\n").await?,
            "LOGOUT" => {
                tx.write_all(b"OK \"Bye\"\r\n").await?;
                return Ok(());
            }
            "AUTHENTICATE" => match handle_authenticate(&state, arg, &mut reader, &mut tx).await? {
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
                let (_name, body) = read_script_literal(arg, &mut reader).await?;
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
                let (_name, body) = read_script_literal(arg, &mut reader).await?;
                match mailrs_sieve::compile_sieve(&body) {
                    Ok(_) => {
                        if store_script(user, &body) {
                            tx.write_all(b"OK \"Script stored\"\r\n").await?;
                        } else {
                            tx.write_all(b"NO \"Storage error\"\r\n").await?;
                        }
                    }
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
                match load_script(user) {
                    Some(body) => {
                        tx.write_all(format!("{{{}}}\r\n", body.len()).as_bytes())
                            .await?;
                        tx.write_all(body.as_bytes()).await?;
                        tx.write_all(b"\r\nOK\r\n").await?;
                    }
                    None => tx.write_all(b"NO \"No such script\"\r\n").await?,
                }
            }
            "LISTSCRIPTS" => {
                let Some(user) = &authed_user else {
                    tx.write_all(b"NO \"Authenticate first\"\r\n").await?;
                    continue;
                };
                if load_script(user).is_some() {
                    tx.write_all(format!("\"{SCRIPT_NAME}\" ACTIVE\r\n").as_bytes())
                        .await?;
                }
                tx.write_all(b"OK \"Listscripts completed\"\r\n").await?;
            }
            "SETACTIVE" => {
                // one-script model: activating the only script is a
                // no-op success; SETACTIVE "" (deactivate) clears it
                let Some(user) = &authed_user else {
                    tx.write_all(b"NO \"Authenticate first\"\r\n").await?;
                    continue;
                };
                let name = unquote(arg);
                if name.is_empty() {
                    delete_script(user);
                }
                tx.write_all(b"OK \"Setactive completed\"\r\n").await?;
            }
            "DELETESCRIPT" => {
                let Some(user) = &authed_user else {
                    tx.write_all(b"NO \"Authenticate first\"\r\n").await?;
                    continue;
                };
                delete_script(user);
                tx.write_all(b"OK \"Deleted\"\r\n").await?;
            }
            "STARTTLS" => {
                // plaintext listener: advertise but no in-band upgrade
                // here (use the TLS listener variant). Politely refuse.
                tx.write_all(b"NO \"STARTTLS not available on this port\"\r\n")
                    .await?;
            }
            _ => {
                tx.write_all(b"NO \"Unknown command\"\r\n").await?;
            }
        }
    }
}

/// AUTHENTICATE PLAIN: either inline base64 arg, or a `{n+}` literal
/// continuation. Returns the authenticated address on success.
async fn handle_authenticate<R, W>(
    state: &Arc<FastcoreState>,
    arg: &str,
    reader: &mut BufReader<R>,
    tx: &mut W,
) -> std::io::Result<Option<String>>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    // arg = `"PLAIN"` optionally followed by the initial response
    let mut it = arg.splitn(2, ' ');
    let mech = unquote(it.next().unwrap_or(""));
    if !mech.eq_ignore_ascii_case("PLAIN") {
        return Ok(None);
    }
    let b64 = match it.next() {
        Some(rest) if !rest.trim().is_empty() => unquote(rest.trim()).to_string(),
        _ => {
            // request the SASL response as a literal
            tx.write_all(b"{0+}\r\n").await?;
            let mut resp = String::new();
            reader.read_line(&mut resp).await?;
            resp.trim_end_matches(['\r', '\n']).to_string()
        }
    };
    use base64::Engine as _;
    let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(b64.trim()) else {
        return Ok(None);
    };
    // SASL PLAIN: authzid \0 authcid \0 passwd
    let mut fields = raw.split(|b| *b == 0);
    let _authzid = fields.next();
    let authcid = fields.next().and_then(|b| std::str::from_utf8(b).ok());
    let passwd = fields.next().and_then(|b| std::str::from_utf8(b).ok());
    match (authcid, passwd) {
        (Some(user), Some(pass)) if crate::imap::backend::verify_password(state, user, pass) => {
            Ok(Some(user.to_string()))
        }
        _ => Ok(None),
    }
}

/// Parse the `{n}` / `{n+}` literal that follows PUTSCRIPT/CHECKSCRIPT
/// and read exactly n bytes. `arg` is `"name" {len+}` or just `{len+}`.
async fn read_script_literal<R>(
    arg: &str,
    reader: &mut BufReader<R>,
) -> std::io::Result<(String, String)>
where
    R: AsyncRead + Unpin,
{
    // extract optional script name + the {len} literal marker
    let name = if arg.trim_start().starts_with('"') {
        let inner = &arg.trim_start()[1..];
        inner.split('"').next().unwrap_or("").to_string()
    } else {
        SCRIPT_NAME.to_string()
    };
    let len: usize = arg
        .rsplit('{')
        .next()
        .and_then(|s| s.trim_end_matches(['}', '+', '\r', '\n']).parse().ok())
        .unwrap_or(0);
    let mut buf = vec![0u8; len];
    tokio::io::AsyncReadExt::read_exact(reader, &mut buf).await?;
    // consume the trailing CRLF after the literal payload
    let mut tail = String::new();
    let _ = reader.read_line(&mut tail).await;
    Ok((name, String::from_utf8_lossy(&buf).into_owned()))
}

fn kevy() -> Option<Connection> {
    let url = std::env::var("MAILRS_KEVY_URL").ok()?;
    Connection::open(&url).ok()
}

fn store_script(user: &str, body: &str) -> bool {
    let Some(mut c) = kevy() else { return false };
    c.set(format!("sieve:{user}").as_bytes(), body.as_bytes())
        .is_ok()
}

fn load_script(user: &str) -> Option<String> {
    let mut c = kevy()?;
    let raw = c.get(format!("sieve:{user}").as_bytes()).ok().flatten()?;
    let s = String::from_utf8(raw).ok()?;
    (!s.trim().is_empty()).then_some(s)
}

fn delete_script(user: &str) {
    if let Some(mut c) = kevy() {
        let _ = c.del(&[format!("sieve:{user}").as_bytes()]);
    }
}

fn unquote(s: &str) -> &str {
    s.trim().trim_matches('"')
}

/// ManageSieve response strings are quoted; strip CR/LF/quote so a
/// compiler error can't break the wire framing.
fn sanitize(s: &str) -> String {
    s.replace(['\r', '\n', '"'], " ")
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
    fn sanitize_neutralizes_framing_chars() {
        assert_eq!(sanitize("bad\r\nscript\"x"), "bad  script x");
    }

    #[test]
    fn greeting_advertises_plain_and_version() {
        let g = greeting();
        assert!(g.contains("\"SASL\" \"PLAIN\""));
        assert!(g.contains("\"VERSION\" \"1.0\""));
        assert!(g.ends_with("OK\r\n"));
    }
}
