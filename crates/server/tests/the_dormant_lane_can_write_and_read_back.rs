//! Two things the pg/spg lane does on nearly every write, checked
//! against the engine it would run on.
//!
//! Both were broken in spg until 7.37.28, and neither could have been
//! noticed here: the lane is dormant, so a defect in it produces no
//! symptom until somebody revives it and finds that account creation,
//! api-key issuance, webhook registration and OIDC code exchange all
//! come back empty.
//!
//! - **`INSERT … RETURNING` described itself as `NoData`** over the
//!   extended protocol, so sqlx received a zero-column row. Present
//!   since v7.9. This lane has **46** `RETURNING` statements, and the
//!   ones that matter return the primary key of the row just written —
//!   `api_key_store`, `webhook::store`, `oidc_store`.
//! - **Binary-format Bind did not cover `jsonb`/`json`** or 1-D arrays.
//!   sqlx binds binary by default, so this is the difference between a
//!   driver-bound JSON column working and not, and the shipped schema
//!   has JSONB on `webhook_outbox`, `messages.invite_payload`,
//!   `calendar_events.attendees` and more.
//!
//! Written against the **shipped schema** and the real tables, not a
//! toy one: a fixture that declares its own columns tests the fixture.

#![cfg(feature = "spg")]

use sqlx::Row;

async fn shipped_schema() -> sqlx::Pool<spg_sqlx::Spg> {
    use spg_sqlx::SpgPoolExt;
    let pool = spg_sqlx::SpgPool::connect_in_memory()
        .await
        .expect("open in-memory spg");
    sqlx::raw_sql(include_str!("../../../scripts/init-schema.sql"))
        .execute(&pool)
        .await
        .expect("apply the shipped schema");
    pool
}

/// `INSERT … RETURNING id` must return the id.
///
/// Shaped exactly as `webhook::store::create` writes it, because the
/// point is the statement this lane actually issues.
#[tokio::test]
async fn insert_returning_gives_back_the_row_it_wrote() {
    let pool = shipped_schema().await;

    let row: (i64,) = sqlx::query_as(
        "INSERT INTO webhook_subscriptions \
         (account_address, url, event_type, filter_sender, filter_thread_id, signing_secret) \
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind("lihao@golia.jp")
    .bind("https://example.test/hook")
    .bind("new_message")
    .bind(None::<String>)
    .bind(None::<String>)
    .bind("s3cret")
    .fetch_one(&pool)
    .await
    .expect("INSERT … RETURNING must produce a row, not NoData");

    assert!(row.0 > 0, "returned id was {}", row.0);

    // And it is the id of the row that now exists — a `RETURNING` that
    // answers with something unrelated would satisfy the line above.
    let found: i64 = sqlx::query_scalar(
        "SELECT id FROM webhook_subscriptions WHERE url = 'https://example.test/hook'",
    )
    .fetch_one(&pool)
    .await
    .expect("select back");
    assert_eq!(row.0, found, "RETURNING id is not the row's id");
}

/// A JSONB column, bound and read back through the driver.
///
/// sqlx binds binary by default, so this exercises the binary Bind path
/// rather than the text one — which is the path that did not cover
/// `jsonb`.
#[tokio::test]
async fn a_jsonb_column_survives_a_driver_bind() {
    let pool = shipped_schema().await;

    let sub: (i64,) = sqlx::query_as(
        "INSERT INTO webhook_subscriptions \
         (account_address, url, event_type, signing_secret) \
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind("lihao@golia.jp")
    .bind("https://example.test/hook2")
    .bind("new_message")
    .bind("s3cret")
    .fetch_one(&pool)
    .await
    .expect("insert subscription");

    // Nested, with a string that would be mangled by a text/binary
    // confusion, so a round trip that "works" by accident does not.
    let payload =
        r#"{"event":"new_message","thread":"t-1","meta":{"n":42,"s":"日本語 \"quoted\""}}"#;
    sqlx::query(
        "INSERT INTO webhook_outbox \
         (subscription_id, payload, next_retry, created_at, updated_at) \
         VALUES ($1, $2::jsonb, $3, $4, $5)",
    )
    .bind(sub.0)
    .bind(payload)
    .bind(0i64)
    .bind(0i64)
    .bind(0i64)
    .execute(&pool)
    .await
    .expect("a driver-bound jsonb value must be accepted");

    let back: String =
        sqlx::query("SELECT payload::text FROM webhook_outbox WHERE subscription_id = $1")
            .bind(sub.0)
            .fetch_one(&pool)
            .await
            .expect("read the payload back")
            .get(0);

    // Compare as parsed JSON: key order and whitespace are the
    // engine's business, the values are ours.
    let sent: serde_json::Value = serde_json::from_str(payload).unwrap();
    let got: serde_json::Value = serde_json::from_str(&back)
        .unwrap_or_else(|e| panic!("payload came back as {back:?}: {e}"));
    assert_eq!(sent, got, "the jsonb value changed on the way through");
}
