//! Both cores' directories, each with one account and nothing else, and
//! the database under the SQL one: spg in memory on the spg axis, a
//! PostgreSQL container on the other.

use std::sync::Arc;

use mailrs_fastcore::{FastcoreDirectory, FastcoreState};

use crate::protocols::PgDirectory;

pub(super) const USER: &str = "alice@example.test";
pub(super) const PASSWORD: &str = "correct horse";
pub(super) const HOST: &str = "mail.example.test";

const SCHEMA_SQL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../scripts/init-schema.sql"
));

#[cfg(feature = "spg")]
async fn database() -> (Box<dyn std::any::Any + Send>, crate::pg::BackendPool) {
    use spg_sqlx::SpgPoolExt;
    let pool = spg_sqlx::SpgPool::connect_in_memory()
        .await
        .expect("open in-memory spg");
    sqlx::raw_sql(SCHEMA_SQL)
        .execute(&pool)
        .await
        .expect("apply init-schema.sql");
    (Box::new(()), pool)
}

#[cfg(not(feature = "spg"))]
async fn database() -> (Box<dyn std::any::Any + Send>, crate::pg::BackendPool) {
    use testcontainers::core::{IntoContainerPort, WaitFor};
    use testcontainers::runners::AsyncRunner;
    use testcontainers::{GenericImage, ImageExt};

    let container = {
        // one container start at a time, workspace-wide
        let _startup = mailrs_test_docker::startup_lock().await;
        GenericImage::new("pgvector/pgvector", "pg18")
            .with_wait_for(WaitFor::message_on_stderr(
                "database system is ready to accept connections",
            ))
            .with_exposed_port(5432.tcp())
            .with_env_var("POSTGRES_PASSWORD", "test")
            .with_env_var("POSTGRES_DB", "mailrs_test")
            .with_env_var("POSTGRES_USER", "postgres")
            .with_startup_timeout(mailrs_test_docker::STARTUP_TIMEOUT)
            .start()
            .await
            .expect("start pgvector")
    };
    let host =
        mailrs_test_docker::ipv4_loopback(&container.get_host().await.expect("host").to_string());
    let port = container.get_host_port_ipv4(5432).await.expect("port");
    let url = format!("postgres://postgres:test@{host}:{port}/mailrs_test");
    // the server may still be restarting after initdb
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let pool = loop {
        match sqlx::PgPool::connect(&url).await {
            Ok(p) => break p,
            Err(_) if std::time::Instant::now() < deadline => {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
            Err(e) => panic!("pg never came up: {e}"),
        }
    };
    sqlx::raw_sql(SCHEMA_SQL)
        .execute(&pool)
        .await
        .expect("apply schema");
    (Box::new(container), pool)
}

pub(super) struct Backends {
    pub fastcore: Arc<FastcoreDirectory>,
    pub pg: Arc<PgDirectory>,
    // a `mem://` store lives only while something has it open
    _kevy: (kevy_client::Connection, kevy_client::Connection),
    _maildirs: (tempfile::TempDir, tempfile::TempDir),
    _db: Box<dyn std::any::Any + Send>,
}

/// The SQL core makes INBOX, Sent, Drafts, Trash and Junk at sign-in; the
/// Maildir core lists the folders that exist. Same folders on both.
const FOLDERS: [&str; 4] = [".Drafts", ".Junk", ".Sent", ".Trash"];

fn fastcore(net_url: &str, root: &std::path::Path) -> Arc<FastcoreDirectory> {
    let store = Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap());
    store.set(b"hostname", HOST.as_bytes()).unwrap();
    let mut state = FastcoreState::new(mailrs_mailbox_kevy::KevyMailboxStore::new(store));
    state.net_url = Some(net_url.to_string());
    let hash = crate::users::UserStore::hash_password(PASSWORD).unwrap();
    let blob = serde_json::json!({ "password_hash": hash }).to_string();
    state.mailbox.upsert_account(USER, &blob).unwrap();
    let home = root.join("example.test").join("alice");
    for folder in FOLDERS {
        for leaf in ["cur", "new", "tmp"] {
            std::fs::create_dir_all(home.join(folder).join(leaf)).unwrap();
        }
    }
    Arc::new(FastcoreDirectory::new(Arc::new(state), root.to_path_buf()))
}

async fn pg(
    net_url: &str,
    root: &std::path::Path,
) -> (Arc<PgDirectory>, Box<dyn std::any::Any + Send>) {
    let (db, pool) = database().await;
    let domain = Arc::new(crate::domain_store::DomainStore::new(
        Some(pool.clone()),
        None,
    ));
    domain.add_domain("example.test", 0).await.unwrap();
    let hash = crate::users::UserStore::hash_password(PASSWORD).unwrap();
    domain
        .add_account(USER, "example.test", "Alice", &hash, 0)
        .await
        .unwrap();
    let dir = PgDirectory {
        mailbox: Arc::new(mailrs_mailbox::PgMailboxStore::new(pool)),
        domain: Some(domain),
        users: Arc::new(crate::users::UserStore::empty()),
        ldap: None,
        message_store: crate::message_store::default_store(),
        events: crate::event_bus::EventBus::new(16),
        maildir_root: root.to_string_lossy().into_owned(),
        hostname: HOST.into(),
        net_url: Some(net_url.to_string()),
    };
    (Arc::new(dir), db)
}

impl Backends {
    /// The account's maildir on the fastcore side (0) or the SQL side (1).
    pub(super) fn home(&self, side: usize) -> std::path::PathBuf {
        let root = match side {
            0 => self._maildirs.0.path(),
            _ => self._maildirs.1.path(),
        };
        root.join("example.test").join("alice")
    }
}

pub(super) async fn start() -> Backends {
    let (fc_url, pg_url) = ("mem://protocols-fastcore", "mem://protocols-pg");
    let kevy = (
        kevy_client::Connection::connect(fc_url).unwrap(),
        kevy_client::Connection::connect(pg_url).unwrap(),
    );
    let maildirs = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let fastcore = fastcore(fc_url, maildirs.0.path());
    let (pg, db) = pg(pg_url, maildirs.1.path()).await;
    Backends {
        fastcore,
        pg,
        _kevy: kevy,
        _maildirs: maildirs,
        _db: db,
    }
}
