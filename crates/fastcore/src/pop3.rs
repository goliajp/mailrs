//! Fastcore-native POP3 server (RFC 1939).
//!
//! Simpler than IMAP by design: a POP3 session is essentially a
//! single-INBOX read-only view where messages are numbered 1..N in
//! scan order and RETR streams the raw file bytes. Optional TOP
//! (RFC 1939 §7) trims to N body lines; DELE marks for delete-at-QUIT.
//!
//! Auth uses the same kevy account store as IMAP.

use std::path::Path;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use crate::FastcoreState;
use crate::imap::backend::{self, ImapMessage};

/// Spawn plaintext POP3 listener on `MAILRS_POP3_BIND` (default
/// `0.0.0.0:110`). Set the env to `off` to disable.
pub async fn spawn(state: Arc<FastcoreState>) {
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
    tracing::info!(%bind, "pop3: listening");
    loop {
        let (sock, peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(error = %e, "pop3: accept error");
                continue;
            }
        };
        let state = state.clone();
        tokio::spawn(async move {
            tracing::debug!(%peer, "pop3: connection open");
            if let Err(e) = session(state, sock).await {
                tracing::debug!(%peer, error = %e, "pop3: session ended");
            }
        });
    }
}

/// Spawn implicit-TLS POP3S listener on `MAILRS_POP3S_BIND`
/// (default `0.0.0.0:995`). Reuses `MAILRS_TLS_CERT` +
/// `MAILRS_TLS_KEY` (same paths as IMAPS + receiver).
pub async fn spawn_tls(state: Arc<FastcoreState>) {
    let bind = std::env::var("MAILRS_POP3S_BIND").unwrap_or_else(|_| "0.0.0.0:995".to_string());
    if bind.eq_ignore_ascii_case("off") || bind.is_empty() {
        tracing::debug!("MAILRS_POP3S_BIND=off — skipping POP3S listener");
        return;
    }
    let (Ok(cert_path), Ok(key_path)) = (
        std::env::var("MAILRS_TLS_CERT"),
        std::env::var("MAILRS_TLS_KEY"),
    ) else {
        tracing::debug!("MAILRS_TLS_CERT / MAILRS_TLS_KEY unset — skipping POP3S listener");
        return;
    };
    let acceptor = match crate::imap::load_tls_acceptor(Path::new(&cert_path), Path::new(&key_path))
    {
        Ok(a) => a,
        Err(e) => {
            tracing::error!(error = %e, %cert_path, %key_path, "pop3s: TLS config load failed");
            return;
        }
    };
    let listener = match TcpListener::bind(&bind).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(error = %e, %bind, "pop3s: bind failed");
            return;
        }
    };
    tracing::info!(%bind, "pop3s: listening (implicit TLS)");
    loop {
        let (sock, peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(error = %e, "pop3s: accept error");
                continue;
            }
        };
        let state = state.clone();
        let acceptor = acceptor.clone();
        tokio::spawn(async move {
            tracing::debug!(%peer, "pop3s: connection open");
            match acceptor.accept(sock).await {
                Ok(tls_sock) => {
                    if let Err(e) = session(state, tls_sock).await {
                        tracing::debug!(%peer, error = %e, "pop3s: session ended");
                    }
                }
                Err(e) => tracing::warn!(%peer, error = %e, "pop3s: handshake failed"),
            }
        });
    }
}

async fn session<S>(state: Arc<FastcoreState>, sock: S) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let (rx, mut tx) = tokio::io::split(sock);
    let mut reader = BufReader::new(rx);
    tx.write_all(b"+OK mailrs POP3 ready\r\n").await?;

    let mut user: Option<String> = None;
    let mut authed_user: Option<String> = None;
    let mut messages: Vec<ImapMessage> = Vec::new();
    let mut deleted: Vec<u32> = Vec::new();

    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await? == 0 {
            break;
        }
        let cleaned = line.trim_end_matches(['\r', '\n']).to_string();
        let mut parts = cleaned.splitn(2, ' ');
        let verb = parts.next().unwrap_or("").to_uppercase();
        let arg = parts.next().unwrap_or("").trim();

        match verb.as_str() {
            "USER" => {
                user = Some(arg.to_string());
                tx.write_all(b"+OK send PASS\r\n").await?;
            }
            "PASS" => {
                let Some(username) = user.clone() else {
                    tx.write_all(b"-ERR USER first\r\n").await?;
                    continue;
                };
                if backend::verify_password(&state, &username, arg) {
                    // Load INBOX messages.
                    let mailboxes = backend::list_mailboxes(&state, &username);
                    if let Some(inbox) = mailboxes.first() {
                        messages = backend::list_messages(inbox);
                    }
                    authed_user = Some(username);
                    tx.write_all(format!("+OK {} messages\r\n", messages.len()).as_bytes())
                        .await?;
                } else {
                    tx.write_all(b"-ERR invalid credentials\r\n").await?;
                }
            }
            "STAT" => {
                if authed_user.is_none() {
                    tx.write_all(b"-ERR not authenticated\r\n").await?;
                    continue;
                }
                let (count, size) = messages
                    .iter()
                    .filter(|m| !deleted.contains(&m.uid))
                    .fold((0u32, 0u64), |(c, s), m| (c + 1, s + m.size));
                tx.write_all(format!("+OK {count} {size}\r\n").as_bytes())
                    .await?;
            }
            "LIST" => {
                if authed_user.is_none() {
                    tx.write_all(b"-ERR not authenticated\r\n").await?;
                    continue;
                }
                if arg.is_empty() {
                    tx.write_all(b"+OK maildrop follows\r\n").await?;
                    for m in &messages {
                        if !deleted.contains(&m.uid) {
                            tx.write_all(format!("{} {}\r\n", m.uid, m.size).as_bytes())
                                .await?;
                        }
                    }
                    tx.write_all(b".\r\n").await?;
                } else {
                    let idx: u32 = arg.parse().unwrap_or(0);
                    match messages.iter().find(|m| m.uid == idx) {
                        Some(m) if !deleted.contains(&idx) => {
                            tx.write_all(format!("+OK {} {}\r\n", m.uid, m.size).as_bytes())
                                .await?;
                        }
                        _ => tx.write_all(b"-ERR no such message\r\n").await?,
                    }
                }
            }
            "UIDL" => {
                if authed_user.is_none() {
                    tx.write_all(b"-ERR not authenticated\r\n").await?;
                    continue;
                }
                tx.write_all(b"+OK unique-id listing follows\r\n").await?;
                for m in &messages {
                    if !deleted.contains(&m.uid) {
                        let uid_str = m
                            .path
                            .file_name()
                            .map(|f| f.to_string_lossy().into_owned())
                            .unwrap_or_else(|| m.uid.to_string());
                        tx.write_all(format!("{} {}\r\n", m.uid, uid_str).as_bytes())
                            .await?;
                    }
                }
                tx.write_all(b".\r\n").await?;
            }
            "RETR" => {
                if authed_user.is_none() {
                    tx.write_all(b"-ERR not authenticated\r\n").await?;
                    continue;
                }
                let idx: u32 = arg.parse().unwrap_or(0);
                let Some(msg) = messages
                    .iter()
                    .find(|m| m.uid == idx && !deleted.contains(&idx))
                else {
                    tx.write_all(b"-ERR no such message\r\n").await?;
                    continue;
                };
                let Some(bytes) = backend::read_message(msg) else {
                    tx.write_all(b"-ERR read failed\r\n").await?;
                    continue;
                };
                tx.write_all(format!("+OK {} octets\r\n", bytes.len()).as_bytes())
                    .await?;
                let stuffed = byte_stuff_pop3(&bytes);
                tx.write_all(&stuffed).await?;
                tx.write_all(b".\r\n").await?;
            }
            "TOP" => {
                if authed_user.is_none() {
                    tx.write_all(b"-ERR not authenticated\r\n").await?;
                    continue;
                }
                let mut sub = arg.splitn(2, ' ');
                let idx: u32 = sub.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                let n: usize = sub.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                let Some(msg) = messages
                    .iter()
                    .find(|m| m.uid == idx && !deleted.contains(&idx))
                else {
                    tx.write_all(b"-ERR no such message\r\n").await?;
                    continue;
                };
                let Some(bytes) = backend::read_message(msg) else {
                    tx.write_all(b"-ERR read failed\r\n").await?;
                    continue;
                };
                let trimmed = trim_to_body_lines(&bytes, n);
                tx.write_all(b"+OK top of message follows\r\n").await?;
                let stuffed = byte_stuff_pop3(&trimmed);
                tx.write_all(&stuffed).await?;
                tx.write_all(b".\r\n").await?;
            }
            "DELE" => {
                if authed_user.is_none() {
                    tx.write_all(b"-ERR not authenticated\r\n").await?;
                    continue;
                }
                let idx: u32 = arg.parse().unwrap_or(0);
                if messages.iter().any(|m| m.uid == idx) {
                    if !deleted.contains(&idx) {
                        deleted.push(idx);
                    }
                    tx.write_all(b"+OK marked for deletion\r\n").await?;
                } else {
                    tx.write_all(b"-ERR no such message\r\n").await?;
                }
            }
            "RSET" => {
                deleted.clear();
                tx.write_all(b"+OK maildrop reset\r\n").await?;
            }
            "NOOP" => {
                tx.write_all(b"+OK\r\n").await?;
            }
            "CAPA" => {
                tx.write_all(
                    b"+OK Capability list follows\r\n\
                    USER\r\nUIDL\r\nTOP\r\nRESP-CODES\r\nSTLS\r\n.\r\n",
                )
                .await?;
            }
            "QUIT" => {
                // Commit pending deletes.
                for &uid in &deleted {
                    if let Some(msg) = messages.iter().find(|m| m.uid == uid) {
                        let _ = backend::delete_file(msg);
                    }
                }
                tx.write_all(b"+OK bye\r\n").await?;
                break;
            }
            _ => {
                tx.write_all(b"-ERR unknown command\r\n").await?;
            }
        }
    }
    Ok(())
}

/// RFC 1939 §3.2 byte-stuffing — any line starting with `.` gets a
/// second `.` prepended, otherwise `.CRLF` would terminate the
/// multiline transfer early.
fn byte_stuff_pop3(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + bytes.len() / 64);
    let mut at_line_start = true;
    for &b in bytes {
        if at_line_start && b == b'.' {
            out.push(b'.');
        }
        out.push(b);
        at_line_start = b == b'\n';
    }
    if !bytes.ends_with(b"\r\n") {
        if !bytes.ends_with(b"\n") {
            out.extend_from_slice(b"\r\n");
        } else {
            // Convert to CRLF ending.
            out.pop();
            out.extend_from_slice(b"\r\n");
        }
    }
    out
}

/// Return headers + first N body lines for the TOP command.
fn trim_to_body_lines(bytes: &[u8], body_lines: usize) -> Vec<u8> {
    let head_end = bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
        .or_else(|| bytes.windows(2).position(|w| w == b"\n\n").map(|p| p + 2))
        .unwrap_or(bytes.len());
    if body_lines == 0 {
        return bytes[..head_end].to_vec();
    }
    let body = &bytes[head_end..];
    let mut collected = 0usize;
    let mut cut = 0usize;
    for (i, &b) in body.iter().enumerate() {
        if b == b'\n' {
            collected += 1;
            cut = i + 1;
            if collected >= body_lines {
                break;
            }
        }
    }
    let mut out = bytes[..head_end].to_vec();
    out.extend_from_slice(&body[..cut]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_stuff_prefixes_dot_lines() {
        let out = byte_stuff_pop3(b"..hidden\r\nnormal\r\n");
        assert!(out.starts_with(b"...hidden"));
    }

    #[test]
    fn byte_stuff_adds_trailing_crlf() {
        let out = byte_stuff_pop3(b"line");
        assert!(out.ends_with(b"line\r\n"));
    }

    #[test]
    fn trim_to_body_lines_zero_returns_headers_only() {
        let raw = b"H1: v\r\nH2: w\r\n\r\nbody 1\r\nbody 2\r\n";
        let out = trim_to_body_lines(raw, 0);
        assert_eq!(out, b"H1: v\r\nH2: w\r\n\r\n");
    }

    #[test]
    fn trim_to_body_lines_returns_n_body_lines() {
        let raw = b"H1: v\r\n\r\nb1\r\nb2\r\nb3\r\n";
        let out = trim_to_body_lines(raw, 1);
        assert!(out.ends_with(b"b1\r\n"));
        assert!(!out.contains(&b'2'));
    }
}
