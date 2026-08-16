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

    // --- and what it does, since 7.37.27 --------------------------
    //
    // This assertion used to pin the opposite: the sweep `migrate-050`
    // documents did **not** reach the index, pinned as observed so that
    // the day spg planned it differently the test would go red and say
    // so. It did, on the 7.37.24 -> 7.37.27 upgrade (2026-08-17), which
    // is the pin working rather than failing.
    //
    // What it was, from their reply: `parse_range_bounds` accepted only a
    // two-sided range, so `col <= x` alone never reached the parser, and
    // the `IS NOT NULL` beside it made the whole predicate unparseable
    // because both conjuncts had to be ranges. The rule was justified as
    // "a one-sided range is usually non-selective" — a guess about a
    // distribution, with a real selectivity cap two functions away, and
    // exactly backwards for this shape: `scheduled_at` is NULL on almost
    // every row and NULLs are not indexed, so the index holds fifty
    // entries out of twenty thousand. Their measurement of our exact
    // query at 160,000 rows: **6.64 ms → 0.014 ms**, with a wide range
    // that matches everything still scanning, as the control.
    //
    // **What to re-verify when the upgrade lands.** Three callers share
    // that parser; two seek and then re-apply the predicate, so a superset
    // is safe, but `count(*)` over an indexed range answers from the index
    // alone and would have counted rows a residual conjunct removes. They
    // split it into a permissive parser and an exact one. That is the
    // shape to check first.
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
        p.contains("Index Scan using idx_outbound_scheduled_at"),
        "the one-sided range no longer reaches the partial index:\n{p}"
    );
    assert!(
        p.contains("Index Cond:"),
        "the bound is being applied as a residual Filter rather than as \n\
         an index condition, which is the shape this started as:\n{p}"
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

/// **The check the upgrade was supposed to be verified by**, written
/// down in the test above before the upgrade landed: *"`count(*)` over
/// an indexed range answers from the index alone and would have counted
/// rows a residual conjunct removes."*
///
/// Two callers of spg's range parser seek and then re-apply the
/// predicate, so a superset costs them nothing. `count(*)` has no such
/// second pass — a permissive parser that hands it one extra key makes
/// it return a number that is simply wrong, with no error and nothing
/// to compare against. spg split the parser in two for this; this is
/// that split, checked from the outside.
///
/// Both halves are required and neither is sufficient. If the plan is a
/// sequential scan the count is trivially right and says nothing about
/// the index; if the count is right but unchecked against the rows, a
/// coincidence passes.
#[tokio::test]
async fn a_count_over_the_indexed_range_counts_only_matching_rows() {
    use spg_sqlx::SpgPoolExt;

    let pool = spg_sqlx::SpgPool::connect_in_memory()
        .await
        .expect("open in-memory spg");
    sqlx::raw_sql(include_str!("../../../scripts/init-schema.sql"))
        .execute(&pool)
        .await
        .expect("apply the shipped schema");

    // Same shape as above: many rows, a handful scheduled, so the index
    // is the cheap answer and the planner will actually take it.
    // `domain` alternates so there is a residual conjunct available that
    // the index knows nothing about.
    for i in 0..20_000i64 {
        sqlx::query(
            "INSERT INTO outbound_queue \
             (sender, recipient, domain, message_data, next_retry, \
              created_at, updated_at, scheduled_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind("s@x.test")
        .bind("r@y.test")
        .bind(if i % 800 == 0 { "a.test" } else { "b.test" })
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

    // Three predicates over the same column: the sweep's one-sided
    // range, a two-sided one, and a range with a conjunct the index
    // cannot answer. The third is the one that catches a permissive
    // parser handing `count` an unfiltered key range.
    for (label, where_clause) in [
        (
            "one-sided, the sweep's own",
            "scheduled_at IS NOT NULL AND scheduled_at <= 1005000",
        ),
        (
            "two-sided",
            "scheduled_at >= 1002000 AND scheduled_at <= 1012000",
        ),
        (
            "range plus a conjunct the index does not carry",
            "scheduled_at IS NOT NULL AND scheduled_at <= 1010000 AND domain = 'a.test'",
        ),
    ] {
        let counted: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM outbound_queue WHERE {where_clause}"
        ))
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| panic!("{label}: count failed: {e}"));

        // The same predicate, as rows. This is the answer `count(*)` is
        // supposed to be an optimisation of.
        let rows: Vec<i64> = sqlx::query_scalar(&format!(
            "SELECT id FROM outbound_queue WHERE {where_clause}"
        ))
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|e| panic!("{label}: row fetch failed: {e}"));

        assert_eq!(
            counted,
            rows.len() as i64,
            "{label}: count({counted}) disagrees with the rows the same \
             predicate returns ({}) — an index-only count is answering \
             for keys the predicate excludes",
            rows.len()
        );
        assert!(
            !rows.is_empty(),
            "{label}: matched nothing, so the comparison above is vacuous"
        );
    }

    // The residual conjunct has to remove something, or the third case
    // above is a range test wearing a disguise: it would agree whether
    // or not the parser is exact, and a fixture change could make it
    // vacuous without anything saying so.
    let with_residual: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbound_queue WHERE scheduled_at IS NOT NULL \
         AND scheduled_at <= 1010000 AND domain = 'a.test'",
    )
    .fetch_one(&pool)
    .await
    .expect("count with residual");
    let without: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbound_queue WHERE scheduled_at IS NOT NULL \
         AND scheduled_at <= 1010000",
    )
    .fetch_one(&pool)
    .await
    .expect("count without residual");
    assert!(
        with_residual < without && with_residual > 0,
        "the residual conjunct removed {} of {without} rows — it has to \
         remove some and leave some, or the case proves nothing",
        without - with_residual
    );

    // And the count really is going through the index — otherwise every
    // assertion above is about a sequential scan and proves nothing.
    let cp = plan(
        &pool,
        "SELECT count(*) FROM outbound_queue \
         WHERE scheduled_at IS NOT NULL AND scheduled_at <= 1005000",
    )
    .await;
    assert!(
        cp.contains("idx_outbound_scheduled_at"),
        "the count did not use the index, so the agreement above says \n\
         nothing about the index-only path:\n{cp}"
    );
}
