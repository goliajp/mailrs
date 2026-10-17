//! The TRANSACTION state of RFC 1939 §5: the INBOX of the signed-in
//! user, numbered by uid, with DELE applied at QUIT.

use std::sync::Arc;

use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::FastcoreState;
use crate::imap::backend::{self, ImapMessage};

pub(super) struct Maildrop {
    messages: Vec<ImapMessage>,
    deleted: Vec<u32>,
}

impl Maildrop {
    pub(super) fn open(state: &Arc<FastcoreState>, user: &str) -> Self {
        let mailboxes = backend::list_mailboxes(state, user);
        let messages = mailboxes
            .first()
            .map(|inbox| backend::list_messages(state, user, inbox))
            .unwrap_or_default();
        Self {
            messages,
            deleted: Vec::new(),
        }
    }

    pub(super) fn len(&self) -> usize {
        self.messages.len()
    }

    pub(super) fn reset(&mut self) {
        self.deleted.clear();
    }

    /// QUIT in the TRANSACTION state.
    pub(super) fn commit(&self) {
        for &uid in &self.deleted {
            if let Some(msg) = self.messages.iter().find(|m| m.uid == uid) {
                let _ = backend::delete_file(msg);
            }
        }
    }

    /// False when `verb` is not a TRANSACTION command.
    pub(super) async fn handle<W>(
        &mut self,
        verb: &str,
        arg: &str,
        tx: &mut W,
    ) -> std::io::Result<bool>
    where
        W: AsyncWrite + Unpin,
    {
        match verb {
            "STAT" => {
                let (count, size) = self
                    .messages
                    .iter()
                    .filter(|m| !self.deleted.contains(&m.uid))
                    .fold((0u32, 0u64), |(c, s), m| (c + 1, s + m.size));
                tx.write_all(format!("+OK {count} {size}\r\n").as_bytes())
                    .await?;
            }
            "LIST" => {
                if arg.is_empty() {
                    tx.write_all(b"+OK maildrop follows\r\n").await?;
                    for m in &self.messages {
                        if !self.deleted.contains(&m.uid) {
                            tx.write_all(format!("{} {}\r\n", m.uid, m.size).as_bytes())
                                .await?;
                        }
                    }
                    tx.write_all(b".\r\n").await?;
                } else {
                    let idx: u32 = arg.parse().unwrap_or(0);
                    match self.messages.iter().find(|m| m.uid == idx) {
                        Some(m) if !self.deleted.contains(&idx) => {
                            tx.write_all(format!("+OK {} {}\r\n", m.uid, m.size).as_bytes())
                                .await?;
                        }
                        _ => tx.write_all(b"-ERR no such message\r\n").await?,
                    }
                }
            }
            "UIDL" => {
                tx.write_all(b"+OK unique-id listing follows\r\n").await?;
                for m in &self.messages {
                    if !self.deleted.contains(&m.uid) {
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
                let idx: u32 = arg.parse().unwrap_or(0);
                let Some(msg) = self
                    .messages
                    .iter()
                    .find(|m| m.uid == idx && !self.deleted.contains(&idx))
                else {
                    tx.write_all(b"-ERR no such message\r\n").await?;
                    return Ok(true);
                };
                let Some(bytes) = backend::read_message(msg) else {
                    tx.write_all(b"-ERR read failed\r\n").await?;
                    return Ok(true);
                };
                tx.write_all(format!("+OK {} octets\r\n", bytes.len()).as_bytes())
                    .await?;
                let stuffed = byte_stuff_pop3(&bytes);
                tx.write_all(&stuffed).await?;
                tx.write_all(b".\r\n").await?;
            }
            "TOP" => {
                let mut sub = arg.splitn(2, ' ');
                let idx: u32 = sub.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                let n: usize = sub.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                let Some(msg) = self
                    .messages
                    .iter()
                    .find(|m| m.uid == idx && !self.deleted.contains(&idx))
                else {
                    tx.write_all(b"-ERR no such message\r\n").await?;
                    return Ok(true);
                };
                let Some(bytes) = backend::read_message(msg) else {
                    tx.write_all(b"-ERR read failed\r\n").await?;
                    return Ok(true);
                };
                let trimmed = trim_to_body_lines(&bytes, n);
                tx.write_all(b"+OK top of message follows\r\n").await?;
                let stuffed = byte_stuff_pop3(&trimmed);
                tx.write_all(&stuffed).await?;
                tx.write_all(b".\r\n").await?;
            }
            "DELE" => {
                let idx: u32 = arg.parse().unwrap_or(0);
                if self.messages.iter().any(|m| m.uid == idx) {
                    if !self.deleted.contains(&idx) {
                        self.deleted.push(idx);
                    }
                    tx.write_all(b"+OK marked for deletion\r\n").await?;
                } else {
                    tx.write_all(b"-ERR no such message\r\n").await?;
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
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
