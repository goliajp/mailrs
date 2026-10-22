//! The shared drain over this store: spool files in, rows out.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use mailrs_core::spool::{SPOOL_SCHEMA_VERSION, SpoolEnvelope, encode_spool_blob};
use spg_sqlx::SpgPoolExt;

use super::{PgCore, PgDirectory};
use crate::pg::BackendPool;

const SCHEMA_SQL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../scripts/init-schema.sql"
));

/// One indexed message, as the views read it.
#[derive(Debug)]
pub(super) struct Indexed {
    pub mailbox: String,
    pub seen: bool,
    pub snippet: String,
    pub thread_id: String,
    pub date: i64,
    pub message_id: String,
    pub category: Option<String>,
}

pub(super) struct Fixture {
    pub pool: BackendPool,
    pub core: Arc<PgCore>,
    pub spool: tempfile::TempDir,
    pub maildir: tempfile::TempDir,
}

impl Fixture {
    pub async fn new() -> Self {
        Self::with_net(None).await
    }

    /// A fixture whose core reaches the network kevy at `net`.
    pub async fn with_net(net: Option<&str>) -> Self {
        let pool = spg_sqlx::SpgPool::connect_in_memory()
            .await
            .expect("open in-memory spg");
        sqlx::raw_sql(SCHEMA_SQL)
            .execute(&pool)
            .await
            .expect("apply init-schema.sql");
        let maildir = tempfile::tempdir().expect("maildir");
        let root = maildir.path().to_string_lossy().into_owned();
        let mailbox = Arc::new(mailrs_mailbox::PgMailboxStore::new(pool.clone()));
        let event_bus = crate::event_bus::EventBus::new(64);
        let domain = Arc::new(crate::domain_store::DomainStore::new(
            Some(pool.clone()),
            None,
        ));
        let core = Arc::new(PgCore {
            dir: PgDirectory::new(domain, root, net.map(str::to_string)),
            mailbox,
            event_bus,
        });
        Self {
            pool,
            core,
            spool: tempfile::tempdir().expect("spool"),
            maildir,
        }
    }

    pub async fn account(&self, addr: &str) {
        let domain = addr.split_once('@').expect("address").1;
        sqlx::query("INSERT INTO domains (name) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(domain)
            .execute(&self.pool)
            .await
            .expect("domain");
        sqlx::query("INSERT INTO accounts (address, domain) VALUES ($1, $2)")
            .bind(addr)
            .bind(domain)
            .execute(&self.pool)
            .await
            .expect("account");
    }

    pub async fn alias(&self, source: &str, target: &str) {
        let domain = source.split_once('@').expect("address").1;
        sqlx::query(
            "INSERT INTO aliases (source_address, target_address, domain) VALUES ($1, $2, $3)",
        )
        .bind(source)
        .bind(target)
        .bind(domain)
        .execute(&self.pool)
        .await
        .expect("alias");
    }

    /// The envelope an outside sender's message to `to` arrives with.
    pub fn envelope(to: &[&str]) -> SpoolEnvelope {
        SpoolEnvelope {
            reverse_path: "carol@outside.example".into(),
            forward_paths: to.iter().map(|s| s.to_string()).collect(),
            is_authenticated: false,
            conn_id: 1,
            target_folder: "INBOX".into(),
            received_at: 1_790_000_000,
            fraud_verdict: None,
            sandbox_ns: None,
            schema_version: SPOOL_SCHEMA_VERSION,
        }
    }

    /// Spool `body` for `to` into `incoming/<dir>` as `name`.
    pub fn spool(&self, dir: &str, name: &str, to: &[&str], body: &[u8]) -> PathBuf {
        self.spool_env(dir, name, &Self::envelope(to), body)
    }

    pub fn spool_env(&self, dir: &str, name: &str, env: &SpoolEnvelope, body: &[u8]) -> PathBuf {
        self.spool_raw(dir, name, &encode_spool_blob(env, body))
    }

    /// The indexed row of `subject` for `user`.
    pub async fn indexed(&self, user: &str, subject: &str) -> Indexed {
        let (mailbox, flags, snippet, thread_id, date, message_id, category): (
            String,
            i32,
            String,
            String,
            i64,
            String,
            Option<String>,
        ) = sqlx::query_as(
            "SELECT mb.name, m.flags, m.snippet, m.thread_id, m.date_epoch, m.message_id,
                    ea.category
               FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
               LEFT JOIN email_analysis ea ON ea.message_id = m.id
              WHERE mb.user_address = $1 AND m.subject = $2",
        )
        .bind(user)
        .bind(subject)
        .fetch_one(&self.pool)
        .await
        .expect("the message was indexed");
        Indexed {
            mailbox,
            seen: flags & 1 != 0,
            snippet,
            thread_id,
            date,
            message_id,
            category,
        }
    }

    pub fn spool_raw(&self, dir: &str, name: &str, bytes: &[u8]) -> PathBuf {
        let at = self.spool.path().join("incoming").join(dir);
        std::fs::create_dir_all(&at).expect("spool dir");
        let path = at.join(name);
        std::fs::write(&path, bytes).expect("spool file");
        path
    }

    /// One drain tick, off the runtime workers as the real loop runs it.
    pub async fn tick(&self) -> usize {
        let core = self.core.clone();
        let dirs = mailrs_intake::spool::SpoolDirs::under(self.spool.path());
        tokio::task::spawn_blocking(move || mailrs_intake::spool::tick(&dirs, &*core))
            .await
            .expect("tick")
    }

    pub fn delivered(&self, addr: &str, name: &str) -> bool {
        let (local, domain) = addr.split_once('@').expect("address");
        maildir_file(self.maildir.path(), domain, local, name).exists()
    }

    /// Wait for the indexed row of `subject` for `user`.
    pub async fn row(&self, user: &str, subject: &str) -> Option<String> {
        for _ in 0..50 {
            let row: Option<(String,)> = sqlx::query_as(
                "SELECT m.maildir_id FROM messages m JOIN mailboxes mb ON m.mailbox_id = mb.id
                  WHERE mb.user_address = $1 AND m.subject = $2",
            )
            .bind(user)
            .bind(subject)
            .fetch_optional(&self.pool)
            .await
            .expect("query");
            if let Some((id,)) = row {
                return Some(id);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        None
    }
}

fn maildir_file(root: &Path, domain: &str, local: &str, name: &str) -> PathBuf {
    root.join(domain).join(local).join("new").join(name)
}

mod arrival;
mod drain;
