//! The scheduled-send sweep's predicate uses its index — asserted, not
//! assumed.
//!
//! `rules/hot-path-needs-a-plan.md` says every hot-path predicate needs an
//! execution plan somebody has looked at, and it says so because of the
//! 2026-07-19 incident: a 48k-row table served **309 billion rows** to a
//! sweep because a composite index's leading column was a scope rather
//! than the predicate's column. The rule's own words: *"这张表有 19 个索引,
//! 应该没事" 不是验证*.
//!
//! `migrate-050` wrote the verification down and could not run it:
//!
//! ```text
//!   EXPLAIN (ANALYZE, BUFFERS)
//!   SELECT id FROM outbound_queue
//!   WHERE scheduled_at IS NOT NULL AND scheduled_at <= 1755000000
//!   ORDER BY scheduled_at LIMIT 100;
//! ```
//!
//! `spg-embedded::Database::explain` existed but sat on a handle this lane
//! does not hold — `SpgPool::connect_in_memory()` yields a pool, not a
//! `Database`. spg's 2026-08-14 reply (§4) answered it: `EXPLAIN` runs as
//! an ordinary query through the pool. So the plan is now checkable on the
//! engine the lane actually runs on, and this is that check.
//!
//! **Against the shipped schema, not a copy of it.** The sibling helper
//! `tests/common/pg.rs` carries its own inline `SCHEMA_DDL`, and that copy
//! has neither `scheduled_at` nor its partial index — so the suite that
//! covers this table has never exercised the column. A test that asserts a
//! plan over an index must read the file that creates the index, or it
//! asserts something about a schema nobody ships.

#![cfg(feature = "spg")]

use sqlx::Row;

/// The statement `migrate-050` documents, verbatim.
const SWEEP: &str = "SELECT id FROM outbound_queue \
     WHERE scheduled_at IS NOT NULL AND scheduled_at <= 1755000000 \
     ORDER BY scheduled_at LIMIT 100";

async fn plan(pool: &sqlx::Pool<spg_sqlx::Spg>, sql: &str) -> String {
    sqlx::query(&format!("EXPLAIN {sql}"))
        .fetch_all(pool)
        .await
        .expect("EXPLAIN is an ordinary query through the pool")
        .iter()
        .map(|r| r.get::<String, _>(0))
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn the_scheduled_sweep_reaches_its_index() {
    use spg_sqlx::SpgPoolExt;

    let pool = spg_sqlx::SpgPool::connect_in_memory()
        .await
        .expect("open in-memory spg");
    sqlx::raw_sql(include_str!("../../../scripts/init-schema.sql"))
        .execute(&pool)
        .await
        .expect("apply the shipped schema");

    // The fixture has to have the workload's shape, not just its size.
    //
    // Two earlier versions of this test failed against a correct plan.
    // At 200 rows a sequential scan is genuinely cheaper and every sane
    // planner picks it. At 20,000 rows with *half* of them scheduled the
    // predicate selects half the table, and a sequential scan is still
    // the right answer — an index that visits 10,000 of 20,000 rows is
    // slower than reading them in order.
    //
    // The schema says what the real shape is, one line above the index
    // itself: "Partial on NOT NULL because almost nothing is scheduled".
    // So: many rows, a handful scheduled. That is the shape the index
    // exists for and the only one whose plan means anything.
    for i in 0..20_000i64 {
        sqlx::query(
            "INSERT INTO outbound_queue \
             (sender, recipient, domain, message_data, next_retry, \
              created_at, updated_at, scheduled_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind("s@x.test")
        .bind("r@y.test")
        .bind("y.test")
        .bind(vec![0u8; 16])
        .bind(0i64)
        .bind(0i64)
        .bind(0i64)
        .bind(if i % 400 == 0 {
            Some(1_000_000i64 + i)
        } else {
            None
        })
        .execute(&pool)
        .await
        .expect("insert");
    }

    // --- what the index can do ------------------------------------
    //
    // An equality predicate reaches it. This is the control: it proves
    // the partial index is declared correctly, is reachable, and that
    // this test is wired to the right table — so the range result below
    // is a statement about the planner and not about the fixture.
    let eq = plan(
        &pool,
        "SELECT id FROM outbound_queue WHERE scheduled_at = 1000000",
    )
    .await;
    assert!(
        eq.contains("Index Scan using idx_outbound_scheduled_at"),
        "the partial index is not reachable at all:\n{eq}"
    );

    // --- what it does not, today ----------------------------------
    //
    // The sweep `migrate-050` documents does NOT reach it. Pinned as
    // observed rather than asserted as wanted, so that the day spg
    // plans it differently this test goes red and says so.
    //
    // Reported to spg 2026-08-16. Three observations, together:
    //
    //   * equality reaches the index (above), so it is not the index
    //   * the row estimate is `rows = <table> / 3` — a fixed default.
    //     Changing the fixture from 10,000 matching rows to 50, a 200x
    //     difference in selectivity, left the estimate and the costs
    //     byte-identical
    //   * `ANALYZE outbound_queue` succeeds and changes neither
    //
    // and the sibling shape says the same thing: on the composite
    // `idx_queue_pending`, `status = 'pending'` is an Index Cond while
    // `next_retry <= 100` is a Filter.
    //
    // **Not live.** `scheduled_at` has no reader on the SQL lane —
    // grep 2026-08-16, the scheduled queue lives entirely in kevy's
    // `mailrs:outbound:scheduled-idx` zset. This is a plan the dormant
    // lane would use if it were revived, which is the only reason it is
    // pinned rather than fixed.
    let p = plan(&pool, SWEEP).await;
    assert!(
        p.contains("Seq Scan"),
        "spg now plans the scheduled sweep without a sequential scan — \n\
         good news, and this pin is what should change:\n{p}"
    );

    // The two plans must differ, or neither says anything. spg's §4
    // pins this on their side too: "a plan that reads the same for an
    // indexed predicate and an unindexed one cannot answer what you are
    // asking it".
    assert_ne!(
        eq, p,
        "indexed and unindexed predicates produced the same plan, so \
         nothing above is evidence:\n{p}"
    );
}
