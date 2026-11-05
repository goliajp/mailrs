//! On the SQL core a deleted account's rows wait for its purge, as
//! fastcore's do: deleting the account row used to cascade to its sieve
//! script, signatures, two-factor secret, calendars and address books at
//! once, so nothing could be restored and the two cores kept different
//! things through the retention period.

use mailrs_core_sidestate::families::account_purge::pending;

use super::harness::{self, Diffs};
use super::{BOB, accounts};

const PG_KEVY: &str = "mem://deleted-rows-pg-core-side-state";

const TABLES: [(&str, &str); 5] = [
    ("sieve_scripts", "address"),
    ("signatures", "account_address"),
    ("totp_secrets", "account_address"),
    ("calendars", "account_address"),
    ("address_books", "account_address"),
];

async fn counts(pool: &crate::pg::BackendPool) -> Vec<(&'static str, i64)> {
    let mut out = Vec::new();
    for (table, column) in TABLES {
        let q = format!("SELECT COUNT(*) FROM {table} WHERE {column} = $1");
        let (n,): (i64,) = sqlx::query_as(&q)
            .bind(BOB)
            .fetch_one(pool)
            .await
            .expect("count");
        out.push((table, n));
    }
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_deleted_accounts_rows_wait_for_its_purge() {
    let cores = harness::start_on("mem://deleted-rows-fastcore-side-state", PG_KEVY).await;
    let mut d = Diffs::default();
    accounts::seed(&cores, &mut d).await;
    let pool = &cores.pg_core_state.pool;
    for q in [
        "INSERT INTO sieve_scripts (address, script) VALUES ($1, 'keep;')",
        "INSERT INTO signatures (account_address, name) VALUES ($1, 'Work')",
        "INSERT INTO totp_secrets (account_address, secret) VALUES ($1, 'S3CRET')",
        "INSERT INTO calendars (account_address) VALUES ($1)",
        "INSERT INTO address_books (account_address) VALUES ($1)",
    ] {
        sqlx::query(q).bind(BOB).execute(pool).await.expect("seed");
    }
    let before = counts(pool).await;
    assert!(before.iter().all(|(_, n)| *n == 1), "{before:?}");

    cores.pg_core.remove_account(BOB).await.expect("delete");
    assert_eq!(
        counts(pool).await,
        before,
        "the delete took rows a restore needs before their purge"
    );

    let mut net = kevy_client::Connection::connect(PG_KEVY).expect("side state");
    pending::clear(&mut net, BOB).expect("clear");
    pending::mark(&mut net, BOB, 1).expect("an old delete");
    cores
        .pg_core
        .account_purge(false, Some(BOB))
        .await
        .expect("purge");
    let after = counts(pool).await;
    assert!(
        after.iter().all(|(_, n)| *n == 0),
        "the purge left {after:?}"
    );
    let (rows,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM accounts WHERE address = $1")
        .bind(BOB)
        .fetch_one(pool)
        .await
        .expect("count");
    assert_eq!(rows, 0, "the purge left the account row");
    assert!(d.0.is_empty(), "{}", d.0.join("\n"));
}
