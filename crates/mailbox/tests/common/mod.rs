//! Shared testcontainers + sqlx setup for mailbox integration tests.
//!
//! Each test that calls [`setup_pg`] gets a fresh Postgres 18 + pgvector
//! container (image: `pgvector/pgvector:pg18`) with the full `init-schema.sql`
//! applied. The returned tuple's first element must be kept alive — dropping
//! the `ContainerAsync` value stops the container.
//!
//! Per-test containers are intentional: total isolation, no fixture
//! contamination between tests. The trade-off is ~3-5 s startup per test;
//! acceptable for the demo-level test count.

#![allow(dead_code)]

use sqlx::PgPool;
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};

const SCHEMA_SQL: &str = include_str!("../../../../scripts/init-schema.sql");

/// Spin up a Postgres + pgvector container, apply `init-schema.sql`, and
/// return both the container handle (must stay alive!) and a connected pool.
pub async fn setup_pg() -> (ContainerAsync<GenericImage>, PgPool) {
    let container = GenericImage::new("pgvector/pgvector", "pg18")
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_exposed_port(5432.tcp())
        .with_env_var("POSTGRES_PASSWORD", "test")
        .with_env_var("POSTGRES_DB", "mailrs_test")
        .with_env_var("POSTGRES_USER", "postgres")
        .start()
        .await
        .expect("failed to start pgvector container");

    let host = container
        .get_host()
        .await
        .expect("container host");
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("container port");
    let url = format!("postgres://postgres:test@{host}:{port}/mailrs_test");

    // Initial container readiness is signaled by the stderr message, but the
    // listener can still race the first connection attempt. Retry briefly.
    let pool = loop_connect(&url, std::time::Duration::from_secs(10)).await;

    sqlx::raw_sql(SCHEMA_SQL)
        .execute(&pool)
        .await
        .expect("apply init-schema.sql");

    (container, pool)
}

async fn loop_connect(url: &str, budget: std::time::Duration) -> PgPool {
    let deadline = std::time::Instant::now() + budget;
    let mut last_err: Option<sqlx::Error> = None;
    while std::time::Instant::now() < deadline {
        match PgPool::connect(url).await {
            Ok(pool) => return pool,
            Err(e) => {
                last_err = Some(e);
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        }
    }
    panic!("pg pool never came up: {last_err:?}");
}

/// Insert a domain + account row so message-level fixtures have FK targets.
pub async fn seed_domain_account(pool: &PgPool, user: &str) {
    let domain = user
        .split_once('@')
        .map(|(_, d)| d)
        .unwrap_or("example.com");

    sqlx::query("INSERT INTO domains (name) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(domain)
        .execute(pool)
        .await
        .expect("seed domain");

    sqlx::query(
        "INSERT INTO accounts (address, domain) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(user)
    .bind(domain)
    .execute(pool)
    .await
    .expect("seed account");
}

/// Insert a mailbox row for `user` and return its id.
pub async fn seed_mailbox(pool: &PgPool, user: &str, name: &str) -> i64 {
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO mailboxes (user_address, name, uidvalidity) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(user)
    .bind(name)
    .bind(1_i32)
    .fetch_one(pool)
    .await
    .expect("seed mailbox");
    row.0
}
