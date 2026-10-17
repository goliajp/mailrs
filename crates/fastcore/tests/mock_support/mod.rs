//! An SMTP client and store probes for the mock mail end-to-end test.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use kevy_client::Connection;
use mailrs_core_sidestate::families::mock;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

pub struct Smtp {
    reader: BufReader<tokio::net::tcp::OwnedReadHalf>,
    writer: tokio::net::tcp::OwnedWriteHalf,
}

impl Smtp {
    pub async fn connect(port: u16) -> Self {
        let mut last = None;
        for _ in 0..100 {
            match TcpStream::connect(("127.0.0.1", port)).await {
                Ok(s) => {
                    let (r, w) = s.into_split();
                    let mut smtp = Self {
                        reader: BufReader::new(r),
                        writer: w,
                    };
                    smtp.reply().await;
                    return smtp;
                }
                Err(e) => last = Some(e),
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("receiver never listened: {last:?}");
    }

    // the final line of a reply, e.g. "250 OK"
    pub async fn reply(&mut self) -> String {
        loop {
            let mut line = String::new();
            self.reader.read_line(&mut line).await.unwrap();
            assert!(!line.is_empty(), "connection closed");
            if line.as_bytes().get(3) != Some(&b'-') {
                return line.trim_end().to_string();
            }
        }
    }

    pub async fn cmd(&mut self, line: &str) -> String {
        self.writer
            .write_all(format!("{line}\r\n").as_bytes())
            .await
            .unwrap();
        self.reply().await
    }

    /// One transaction; the reply to the end of DATA, or to the first
    /// command refused before it.
    pub async fn send(&mut self, from: &str, to: &[&str], body: &str) -> String {
        let r = self.cmd(&format!("MAIL FROM:<{from}>")).await;
        if !r.starts_with('2') {
            return r;
        }
        for rcpt in to {
            let r = self.cmd(&format!("RCPT TO:<{rcpt}>")).await;
            if !r.starts_with('2') {
                return r;
            }
        }
        assert!(self.cmd("DATA").await.starts_with("354"));
        self.cmd(&format!("{body}\r\n.")).await
    }
}

pub fn keys(c: &mut Connection) -> BTreeSet<String> {
    c.keys(b"*")
        .unwrap()
        .into_iter()
        .map(|k| String::from_utf8_lossy(&k).into_owned())
        .collect()
}

pub fn spool_files(dir: &Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

pub fn all(c: &mut Connection, ns: &str) -> Vec<mock::MessageMeta> {
    let f = mock::Filter {
        limit: usize::MAX,
        ..Default::default()
    };
    mock::list_messages(c, ns, &f).unwrap()
}

pub fn files_under(dir: &Path) -> usize {
    walk(dir).len()
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
    }
    out
}
