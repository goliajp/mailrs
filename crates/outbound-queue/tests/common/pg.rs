//! Shared Postgres fixture for outbound-queue integration tests.
//!
//! Starts an ephemeral `postgres:17-alpine` container per call, applies the
//! `outbound_queue` + `suppression_list` DDL subset our code touches, and
//! returns a connected `PgPool` alongside the container handle. The handle
//! MUST be kept alive for the lifetime of the pool — dropping it stops the
//! container.
//!
//! Tests using this fixture MUST run with `--test-threads=1` because
//! testcontainers binds ephemeral host ports and Docker's per-container
//! cold start (~3-5s) doesn't parallelise meaningfully on a laptop.

use mailrs_outbound_queue::BackendPool;
#[cfg(not(feature = "spg"))]
use sqlx::postgres::PgPoolOptions;
#[cfg(not(feature = "spg"))]
use testcontainers::ContainerAsync;
#[cfg(not(feature = "spg"))]
use testcontainers_modules::postgres::Postgres;
#[cfg(not(feature = "spg"))]
use testcontainers_modules::testcontainers::runners::AsyncRunner;

/// Keep-alive handle for the backing store. The PG axis must hold the
/// container; the spg axis has nothing to keep alive.
#[cfg(not(feature = "spg"))]
pub type TestHandle = ContainerAsync<Postgres>;
/// Keep-alive handle for the backing store. The PG axis must hold the
/// container; the spg axis has nothing to keep alive.
#[cfg(feature = "spg")]
pub type TestHandle = ();

/// DDL applied to every fresh container. Mirrors `scripts/init-schema.sql`
/// (the `outbound_queue` table + its pending index) and
/// `scripts/migrate-027-suppression-list.sql` (the `suppression_list`
/// table + its two indexes). Kept inline so a schema drift in the prod
/// scripts is caught the moment a test runs against the real PgStore.
const SCHEMA_DDL: &str = r#"
CREATE TABLE outbound_queue (
    id BIGSERIAL PRIMARY KEY,
    sender TEXT NOT NULL,
    recipient TEXT NOT NULL,
    domain TEXT NOT NULL,
    message_data TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 8,
    next_retry BIGINT NOT NULL,
    last_error TEXT,
    message_id TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    is_forwarded BOOLEAN NOT NULL DEFAULT false
);
CREATE INDEX idx_queue_pending ON outbound_queue(status, next_retry)
    WHERE status = 'pending';

CREATE TABLE suppression_list (
    id BIGSERIAL PRIMARY KEY,
    email TEXT NOT NULL,
    reason TEXT NOT NULL DEFAULT '',
    bounce_type TEXT NOT NULL DEFAULT 'hard',
    smtp_code INTEGER,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX idx_suppression_email ON suppression_list (email);
CREATE INDEX idx_suppression_created ON suppression_list (created_at DESC);
"#;

/// Spin a fresh Postgres container, return its handle plus a connected
/// pool with `outbound_queue` + `suppression_list` already migrated.
///
/// # Panics
///
/// Panics on container-start or DDL-execution failure. Integration tests
/// would be meaningless if the fixture is half-up, so we fail loudly.
#[cfg(feature = "spg")]
pub async fn start_pg() -> (TestHandle, BackendPool) {
    use spg_sqlx::SpgPoolExt;
    let pool = spg_sqlx::SpgPool::connect_in_memory()
        .await
        .expect("open in-memory spg");
    sqlx::raw_sql(SCHEMA_DDL)
        .execute(&pool)
        .await
        .expect("apply schema DDL");
    ((), pool)
}

#[cfg(not(feature = "spg"))]
pub async fn start_pg() -> (TestHandle, BackendPool) {
    let container = Postgres::default()
        .start()
        .await
        .expect("start postgres container");

    let host = container
        .get_host()
        .await
        .expect("get container host")
        .to_string();
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("get container port");
    let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");

    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&url)
        .await
        .expect("connect to postgres");

    sqlx::raw_sql(SCHEMA_DDL)
        .execute(&pool)
        .await
        .expect("apply schema DDL");

    (container, pool)
}
