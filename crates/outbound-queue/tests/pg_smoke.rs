//! Smoke test: container starts, schema applies, `SELECT 1` round-trips.
//! Sanity check for `tests/common/pg.rs` before any real lifecycle tests.

mod common;

use common::pg::start_pg;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pg_fixture_starts_and_responds() {
    let (_container, pool) = start_pg().await;

    let (one,): (i32,) = sqlx::query_as("SELECT 1")
        .fetch_one(&pool)
        .await
        .expect("SELECT 1");
    assert_eq!(one, 1);

    let (count,): (i64,) = sqlx::query_as("SELECT count(*) FROM outbound_queue")
        .fetch_one(&pool)
        .await
        .expect("count outbound_queue");
    assert_eq!(count, 0, "fresh container starts with empty queue");

    let (sc,): (i64,) = sqlx::query_as("SELECT count(*) FROM suppression_list")
        .fetch_one(&pool)
        .await
        .expect("count suppression_list");
    assert_eq!(sc, 0, "fresh container starts with empty suppression list");
}
