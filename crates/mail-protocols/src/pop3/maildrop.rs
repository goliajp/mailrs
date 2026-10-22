//! The TRANSACTION state of RFC 1939 §5: the INBOX of the signed-in
//! user, numbered from 1 in uid order, with DELE applied at QUIT.

use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::backend::{Account, Error, Message};

pub(super) struct Maildrop<A: Account> {
    account: A,
    inbox: Option<A::Mailbox>,
    messages: Vec<Message<A::Key>>,
    deleted: Vec<u32>,
}

impl<A: Account> Maildrop<A> {
    pub(super) async fn open(account: A) -> Result<Self, Error> {
        let inbox = account.mailbox("INBOX").await?;
        let messages = match &inbox {
            Some(mb) => account.messages(mb).await?,
            None => Vec::new(),
        };
        Ok(Self {
            account,
            inbox,
            messages,
            deleted: Vec::new(),
        })
    }

    pub(super) fn len(&self) -> usize {
        self.messages.len()
    }

    pub(super) fn reset(&mut self) {
        self.deleted.clear();
    }

    /// QUIT in the TRANSACTION state. Stops at the first message that
    /// cannot be removed.
    pub(super) async fn commit(&self) -> Result<(), Error> {
        let Some(inbox) = &self.inbox else {
            return Ok(());
        };
        for &n in &self.deleted {
            if let Some(msg) = self.get(n) {
                self.account.remove(inbox, msg).await?;
            }
        }
        Ok(())
    }

    /// Message `n`, counting from 1, whether or not it is marked deleted.
    fn get(&self, n: u32) -> Option<&Message<A::Key>> {
        let i = usize::try_from(n).ok()?.checked_sub(1)?;
        self.messages.get(i)
    }

    fn live(&self, n: u32) -> Option<&Message<A::Key>> {
        self.get(n).filter(|_| !self.deleted.contains(&n))
    }

    fn numbered(&self) -> impl Iterator<Item = (u32, &Message<A::Key>)> {
        (1u32..)
            .zip(&self.messages)
            .filter(|(n, _)| !self.deleted.contains(n))
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
                    .numbered()
                    .fold((0u32, 0u64), |(c, s), (_, m)| (c + 1, s + m.size));
                tx.write_all(format!("+OK {count} {size}\r\n").as_bytes())
                    .await?;
            }
            "LIST" => self.list(arg, tx).await?,
            "UIDL" => {
                tx.write_all(b"+OK unique-id listing follows\r\n").await?;
                for (n, m) in self.numbered() {
                    let id = self.account.unique_id(m);
                    tx.write_all(format!("{n} {id}\r\n").as_bytes()).await?;
                }
                tx.write_all(b".\r\n").await?;
            }
            "RETR" => self.send(arg.parse().unwrap_or(0), None, tx).await?,
            "TOP" => {
                let mut sub = arg.splitn(2, ' ');
                let idx: u32 = sub.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                let n: usize = sub.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                self.send(idx, Some(n), tx).await?;
            }
            "DELE" => {
                let idx: u32 = arg.parse().unwrap_or(0);
                if self.get(idx).is_some() {
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

    async fn list<W: AsyncWrite + Unpin>(&self, arg: &str, tx: &mut W) -> std::io::Result<()> {
        if !arg.is_empty() {
            let idx: u32 = arg.parse().unwrap_or(0);
            return match self.live(idx) {
                Some(m) => {
                    tx.write_all(format!("+OK {idx} {}\r\n", m.size).as_bytes())
                        .await
                }
                None => tx.write_all(b"-ERR no such message\r\n").await,
            };
        }
        tx.write_all(b"+OK maildrop follows\r\n").await?;
        for (n, m) in self.numbered() {
            tx.write_all(format!("{n} {}\r\n", m.size).as_bytes())
                .await?;
        }
        tx.write_all(b".\r\n").await
    }

    /// RETR, or TOP with `top` body lines.
    async fn send<W: AsyncWrite + Unpin>(
        &self,
        idx: u32,
        top: Option<usize>,
        tx: &mut W,
    ) -> std::io::Result<()> {
        let Some(msg) = self.live(idx) else {
            return tx.write_all(b"-ERR no such message\r\n").await;
        };
        let bytes = match self.account.read(msg).await {
            Ok(Some(b)) => b,
            Ok(None) => return tx.write_all(b"-ERR read failed\r\n").await,
            Err(e) => {
                tracing::warn!(err = %e, "pop3: read failed");
                return tx.write_all(b"-ERR read failed\r\n").await;
            }
        };
        let body = match top {
            Some(n) => {
                tx.write_all(b"+OK top of message follows\r\n").await?;
                trim_to_body_lines(&bytes, n)
            }
            None => {
                tx.write_all(format!("+OK {} octets\r\n", bytes.len()).as_bytes())
                    .await?;
                bytes
            }
        };
        tx.write_all(&byte_stuff_pop3(&body)).await?;
        tx.write_all(b".\r\n").await
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
