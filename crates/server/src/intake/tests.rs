//! The shared drain over this store: spool files in, rows out.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use mailrs_core::spool::{SPOOL_SCHEMA_VERSION, SpoolEnvelope, encode_spool_blob};
use spg_sqlx::SpgPoolExt;

use super::{Delivery, PgCore, PgDirectory};
use crate::pg::BackendPool;

const SCHEMA_SQL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../scripts/init-schema.sql"
));

pub(super) struct Fixture {
    pub pool: BackendPool,
    pub core: Arc<PgCore>,
    pub spool: tempfile::TempDir,
    pub maildir: tempfile::TempDir,
}

impl Fixture {
    pub async fn new() -> Self {
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
        let process_tx = crate::smtp_session::spawn_process_consumer(Some(Arc::new(
            crate::smtp_session::ProcessDeps {
                mailbox_store: mailbox,
                event_bus: event_bus.clone(),
                outbound_queue: None,
                resolver: None,
                maildir_root: root.clone(),
                kevy_url: None,
            },
        )));
        let domain = Arc::new(crate::domain_store::DomainStore::new(
            Some(pool.clone()),
            None,
            crate::health::HealthState::new(),
        ));
        let core = Arc::new(PgCore {
            dir: PgDirectory::new(domain, root, None),
            delivery: Delivery {
                process_tx,
                account_store: None,
                outbound_enqueue: None,
                queue_notifier: None,
                event_bus,
                hostname: "mx.test.local".into(),
                srs_secret: None,
            },
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

    /// Spool `body` for `to` into `incoming/<dir>` as `name`.
    pub fn spool(&self, dir: &str, name: &str, to: &[&str], body: &[u8]) -> PathBuf {
        let env = SpoolEnvelope {
            reverse_path: "carol@outside.example".into(),
            forward_paths: to.iter().map(|s| s.to_string()).collect(),
            is_authenticated: false,
            conn_id: 1,
            target_folder: "INBOX".into(),
            received_at: 1_790_000_000,
            fraud_verdict: None,
            sandbox_ns: None,
            schema_version: SPOOL_SCHEMA_VERSION,
        };
        self.spool_raw(dir, name, &encode_spool_blob(&env, body))
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

const BODY: &[u8] = b"From: Carol <carol@outside.example>\r\n\
To: alice@example.com\r\n\
Subject: drained\r\n\
Message-ID: <drained-1@outside.example>\r\n\
\r\n\
hello\r\n";

/// A file in `cur` is drained like one in `new`, and an alias reaches its
/// target's mailbox.
#[tokio::test(flavor = "multi_thread")]
async fn an_alias_in_cur_reaches_its_target() {
    let f = Fixture::new().await;
    f.account("alice@example.com").await;
    f.alias("contact@example.com", "alice@example.com").await;
    let file = f.spool(
        "cur",
        "1790000000.M1P1Q1.host",
        &["Contact@Example.com"],
        BODY,
    );

    assert_eq!(f.tick().await, 1);
    assert!(!file.exists(), "a delivered file stays in the spool");
    assert!(f.delivered("alice@example.com", "1790000000.M1P1Q1.host"));
    assert_eq!(
        f.row("alice@example.com", "drained").await.as_deref(),
        Some("1790000000.M1P1Q1.host")
    );
}

/// Nobody takes mail at an address with no mailbox, alias or group: the
/// file stays for a human, and no mailbox is made for it.
#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_recipient_leaves_the_file_and_makes_no_mailbox() {
    let f = Fixture::new().await;
    f.account("alice@example.com").await;
    let file = f.spool(
        "new",
        "1790000001.M1P1Q1.host",
        &["ghost@example.com"],
        BODY,
    );

    assert_eq!(f.tick().await, 0);
    assert!(file.exists(), "an undeliverable file was removed");
    assert!(!f.maildir.path().join("example.com").join("ghost").exists());
}

/// A file that does not decode is kept, never deleted.
#[tokio::test(flavor = "multi_thread")]
async fn an_undecodable_file_is_kept() {
    let f = Fixture::new().await;
    let file = f.spool_raw("new", "garbage", b"not a spool envelope");
    assert_eq!(f.tick().await, 0);
    assert!(file.exists());
}
