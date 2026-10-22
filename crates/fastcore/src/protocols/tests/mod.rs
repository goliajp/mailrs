//! The shared sessions over fastcore's own stores: what only this
//! storage can get right or wrong.

mod mock;
mod moves;
mod pop3;
mod seen;
mod sieve;
mod uidvalidity;
mod writes;

use std::path::Path;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf};

use super::FastcoreDirectory;
use crate::FastcoreState;

pub(super) const USER: &str = "alice@golia.example.test";
pub(super) const PASSWORD: &str = "correct horse";

/// A state holding [`USER`], with mail under `root`.
pub(super) fn directory(state: FastcoreState, root: &Path) -> Arc<FastcoreDirectory> {
    let hash = mailrs_core::users::UserStore::hash_password(PASSWORD).unwrap();
    state
        .mailbox
        .upsert_account(
            USER,
            &serde_json::json!({ "password_hash": hash }).to_string(),
        )
        .unwrap();
    Arc::new(FastcoreDirectory {
        state: Arc::new(state),
        maildir_root: root.to_path_buf(),
    })
}

pub(super) fn state() -> FastcoreState {
    FastcoreState::new(mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    )))
}

/// [`USER`]'s INBOX under `root`, holding `files` (paths below it).
pub(super) fn inbox(root: &Path, files: &[&str]) -> std::path::PathBuf {
    let dir = root.join("golia.example.test").join("alice");
    for leaf in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(dir.join(leaf)).unwrap();
    }
    for (i, name) in files.iter().enumerate() {
        let raw = format!("Message-ID: <m{i}@test>\r\nSubject: t\r\n\r\nbody {i}\r\n");
        std::fs::write(dir.join(name), raw).unwrap();
    }
    dir
}

pub(super) struct Imap {
    reader: BufReader<ReadHalf<DuplexStream>>,
    writer: WriteHalf<DuplexStream>,
    n: u32,
}

impl Imap {
    pub(super) async fn connect(dir: &Arc<FastcoreDirectory>) -> Self {
        let (client, server) = tokio::io::duplex(1 << 20);
        tokio::spawn(mailrs_mail_protocols::imap::run(dir.clone(), server));
        let (r, w) = tokio::io::split(client);
        let mut imap = Self {
            reader: BufReader::new(r),
            writer: w,
            n: 0,
        };
        let mut greeting = String::new();
        imap.reader.read_line(&mut greeting).await.unwrap();
        assert!(greeting.starts_with("* OK"), "{greeting}");
        imap
    }

    /// Everything the server sent back, the tagged line last.
    pub(super) async fn cmd(&mut self, line: &str) -> String {
        self.n += 1;
        let tag = format!("a{}", self.n);
        self.writer
            .write_all(format!("{tag} {line}\r\n").as_bytes())
            .await
            .unwrap();
        let mut out = String::new();
        loop {
            let mut l = String::new();
            self.reader.read_line(&mut l).await.unwrap();
            assert!(!l.is_empty(), "connection closed after {line}: {out}");
            out.push_str(&l);
            if l.starts_with(&format!("{tag} ")) {
                return out;
            }
        }
    }
}

pub(super) fn tagged(reply: &str) -> &str {
    reply.lines().last().unwrap_or("")
}
