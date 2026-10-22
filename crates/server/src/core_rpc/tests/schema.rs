//! The SQL core creates its schema on an empty database, leaves a current
//! one alone, and refuses one it cannot place — on both engines.

use crate::pg::BackendPool;
use crate::pg_schema::{SCHEMA_VERSION, ensure};

#[cfg(feature = "spg")]
async fn empty_db() -> (Box<dyn std::any::Any + Send>, BackendPool) {
    use spg_sqlx::SpgPoolExt;
    let pool = spg_sqlx::SpgPool::connect_in_memory().await.expect("spg");
    (Box::new(()), pool)
}

#[cfg(not(feature = "spg"))]
async fn empty_db() -> (Box<dyn std::any::Any + Send>, BackendPool) {
    let (container, pool) = super::real_pg_sync::empty_pg().await;
    (Box::new(container), pool)
}

async fn version(pool: &BackendPool) -> i32 {
    let (v,): (i32,) = sqlx::query_as("SELECT MAX(version) FROM schema_version")
        .fetch_one(pool)
        .await
        .expect("schema_version");
    v
}

#[tokio::test]
async fn an_empty_database_gets_the_schema_once() {
    let (_db, pool) = empty_db().await;
    ensure(&pool).await.expect("create");
    assert_eq!(version(&pool).await, SCHEMA_VERSION);
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM accounts")
        .fetch_one(&pool)
        .await
        .expect("accounts exists");
    assert_eq!(n, 0);
    ensure(&pool).await.expect("a current schema is left alone");
    assert_eq!(version(&pool).await, SCHEMA_VERSION);
}

#[tokio::test]
async fn a_database_it_cannot_place_is_refused() {
    let (_db, pool) = empty_db().await;
    sqlx::raw_sql("CREATE TABLE accounts (address TEXT)")
        .execute(&pool)
        .await
        .expect("seed");
    let e = ensure(&pool).await.expect_err("tables without a version");
    assert!(e.contains("no schema_version"), "{e}");

    sqlx::raw_sql(&format!(
        "CREATE TABLE schema_version (version INTEGER NOT NULL); \
         INSERT INTO schema_version (version) VALUES ({})",
        SCHEMA_VERSION + 1
    ))
    .execute(&pool)
    .await
    .expect("seed");
    let e = ensure(&pool).await.expect_err("a newer database");
    assert!(e.contains("newer than this binary"), "{e}");
}
