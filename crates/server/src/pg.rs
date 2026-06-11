//! SQL backend pool — PostgreSQL by default, spg-embedded behind the
//! `spg` feature. One alias set; everything downstream types against
//! [`BackendPool`] and stays backend-agnostic.

/// Connection pool of the active backend.
#[cfg(not(feature = "spg"))]
pub type BackendPool = sqlx::PgPool;
/// Connection pool of the active backend.
#[cfg(feature = "spg")]
pub type BackendPool = spg_sqlx::SpgPool;

/// Connect a PostgreSQL pool from a `postgres://` URL.
#[cfg(not(feature = "spg"))]
pub async fn create_pool(url: &str) -> Result<BackendPool, sqlx::Error> {
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(20)
        .min_connections(2)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .idle_timeout(std::time::Duration::from_secs(600))
        .max_lifetime(std::time::Duration::from_secs(1800))
        .test_before_acquire(true)
        .connect(url)
        .await
}

/// Open the in-process spg-embedded engine. `MAILRS_PG_URL` keeps its
/// role as the single DB locator — `spg:///data/spg/mailrs.db` opens
/// (or creates) that catalog file. No TCP, no container.
#[cfg(feature = "spg")]
pub async fn create_pool(url: &str) -> Result<BackendPool, sqlx::Error> {
    let opts: spg_sqlx::SpgConnectOptions = url
        .parse()
        .map_err(|e| sqlx::Error::Configuration(format!("{e}").into()))?;
    spg_sqlx::SpgPoolOptions::new()
        .max_connections(20)
        .connect_with(opts)
        .await
}

/// Force-release the catalog lock before opening (spg build only).
/// Safe ONLY under the single-instance deployment contract — the
/// caller asserts no other process can hold this catalog. Used when
/// `MAILRS_SPG_FORCE_UNLOCK` is set (docker compose: a SIGKILLed
/// predecessor's lock is unreclaimable across container namespaces).
#[cfg(feature = "spg")]
pub fn force_unlock(url: &str) {
    let Some(path) = url.strip_prefix("spg://") else {
        return;
    };
    match spg_embedded::Database::force_unlock(path) {
        Ok(()) => {
            tracing::info!(
                path,
                "spg catalog lock force-released (single-instance contract)"
            );
        }
        Err(e) => tracing::warn!(path, error = ?e, "spg force_unlock failed"),
    }
}
