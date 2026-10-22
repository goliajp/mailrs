//! The schema the SQL core runs on: created on an empty database, brought
//! forward on one that is behind, and refused when it cannot be placed.

use crate::pg::BackendPool;

/// The whole schema, equal to every `scripts/migrate-NNN-*.sql` up to
/// [`SCHEMA_VERSION`].
const SCHEMA_SQL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../scripts/init-schema.sql"
));

/// The last migration [`SCHEMA_SQL`] includes.
pub(crate) const SCHEMA_VERSION: i32 = 51;

/// Migrations after [`SCHEMA_VERSION`], in order: `(version, sql)`.
const MIGRATIONS: &[(i32, &str)] = &[];

/// Make the database's schema the one this binary was built for.
pub(crate) async fn ensure(pool: &BackendPool) -> Result<(), String> {
    let fail = |e: sqlx::Error| e.to_string();
    if !table_exists(pool, "schema_version").await.map_err(fail)? {
        if table_exists(pool, "accounts").await.map_err(fail)? {
            return Err(format!(
                "the database has tables but no schema_version: bring it to migration \
                 {SCHEMA_VERSION} with scripts/migrate-*.sql (the last one records the version)"
            ));
        }
        sqlx::raw_sql(SCHEMA_SQL)
            .execute(pool)
            .await
            .map_err(fail)?;
        tracing::info!(
            version = SCHEMA_VERSION,
            "sql schema created on an empty database"
        );
        return Ok(());
    }
    let (at,): (i32,) = sqlx::query_as("SELECT MAX(version) FROM schema_version")
        .fetch_one(pool)
        .await
        .map_err(fail)?;
    let newest = MIGRATIONS.last().map_or(SCHEMA_VERSION, |(v, _)| *v);
    if at > newest {
        return Err(format!(
            "the database is at migration {at}, newer than this binary's {newest}"
        ));
    }
    for (version, sql) in MIGRATIONS.iter().filter(|(v, _)| *v > at) {
        sqlx::raw_sql(sql).execute(pool).await.map_err(fail)?;
        sqlx::query("UPDATE schema_version SET version = $1")
            .bind(*version)
            .execute(pool)
            .await
            .map_err(fail)?;
        tracing::info!(version, "sql migration applied");
    }
    Ok(())
}

async fn table_exists(pool: &BackendPool, name: &str) -> Result<bool, sqlx::Error> {
    let (n,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM information_schema.tables WHERE table_name = $1")
            .bind(name)
            .fetch_one(pool)
            .await?;
    Ok(n > 0)
}

#[cfg(test)]
mod tests {
    /// The embedded schema claims the newest migration in the tree, so a
    /// migration added without folding it into the schema is caught here.
    #[test]
    fn the_schema_is_the_newest_migration() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts");
        let newest = std::fs::read_dir(dir)
            .expect("scripts/")
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter_map(|n| n.strip_prefix("migrate-")?.get(..3)?.parse::<i32>().ok())
            .max()
            .expect("a migration");
        assert_eq!(newest, super::SCHEMA_VERSION);
        assert!(super::SCHEMA_SQL.contains(&format!("VALUES ({})", super::SCHEMA_VERSION)));
    }
}
