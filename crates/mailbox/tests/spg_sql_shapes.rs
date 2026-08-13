//! Which standard-SQL shapes the embedded engine can parse.
//!
//! One workaround in this crate is keyed to an engine version:
//! `count_unseen` was rewritten from a FROM-clause derived table plus an
//! aggregate `FILTER` into a CTE plus `COUNT(CASE …)` because spg 7.30.3
//! could parse neither. Both forms are standard SQL and equivalent; the
//! rewrite exists only to work around the parser of the day.
//!
//! A comment cannot tell you when it has stopped being needed. This asks the
//! engine directly, so the workaround's continued existence is a test result
//! rather than a note somebody has to remember to re-check.
//!
//! spg-only: on PostgreSQL both shapes have always parsed, so there is nothing
//! to learn from running it there.

#![cfg(feature = "spg")]

mod common;

/// Does `FILTER (WHERE …)` on an aggregate parse?
///
/// This is half of what `count_unseen`'s rewrite was for.
#[tokio::test]
async fn aggregate_filter_clause() {
    let (_h, pool) = common::setup_pg().await;
    let r: Result<(i64,), _> = sqlx::query_as(
        "SELECT COUNT(*) FILTER (WHERE (m.flags & 1) = 0)
         FROM messages m",
    )
    .fetch_one(&pool)
    .await;
    report("aggregate FILTER", &r);
}

/// Does a FROM-clause derived table parse?
///
/// The other half. `count_unseen` originally grouped in a subquery and
/// counted its rows.
#[tokio::test]
async fn from_clause_derived_table() {
    let (_h, pool) = common::setup_pg().await;
    let r: Result<(i64,), _> = sqlx::query_as(
        "SELECT COUNT(*) FROM (
           SELECT m.thread_id FROM messages m
           WHERE m.thread_id != ''
           GROUP BY m.thread_id
         ) t",
    )
    .fetch_one(&pool)
    .await;
    report("FROM-clause derived table", &r);
}

/// The shape the workaround uses today, as the control.
///
/// If this ever fails the workaround itself has stopped parsing, which is a
/// different and more urgent problem than the two above.
#[tokio::test]
async fn cte_plus_count_case_still_parses() {
    let (_h, pool) = common::setup_pg().await;
    let r: Result<(i64,), _> = sqlx::query_as(
        "WITH t AS (
           SELECT m.thread_id FROM messages m
           WHERE m.thread_id != ''
           GROUP BY m.thread_id
           HAVING COUNT(CASE WHEN (m.flags & 1) = 0 THEN 1 END) > 0
         )
         SELECT COUNT(*) FROM t",
    )
    .fetch_one(&pool)
    .await;
    assert!(
        r.is_ok(),
        "the shape count_unseen actually uses no longer parses: {:?}",
        r.err()
    );
}

/// Print the verdict and assert nothing about the two probes.
///
/// Deliberately not an assertion in either direction. Asserting they *fail*
/// would break the day the engine gains them — which is the day someone wants
/// to know. Asserting they *pass* would fail on an engine that has not got
/// there yet, and this crate does not choose its engine version. The output is
/// the deliverable: run
/// `cargo test -p mailrs-mailbox --features spg --test spg_sql_shapes -- --nocapture`
/// and read it.
fn report<T>(shape: &str, r: &Result<T, sqlx::Error>) {
    match r {
        Ok(_) => println!(
            "PARSES: {shape} — `count_unseen`'s CTE rewrite can go back to the \
             plainer form (crates/mailbox/src/pg/message_ops/read.rs)"
        ),
        Err(e) => println!("STILL UNSUPPORTED: {shape} — {e}"),
    }
}
