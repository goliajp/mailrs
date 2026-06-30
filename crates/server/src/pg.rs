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

/// Connect the backend pool, retrying with capped backoff before
/// conceding to degraded mode. A fresh boot can race the engine's
/// WAL replay: a restart with accumulated WAL keeps the engine busy
/// for a minute or two, during which connection establishment fails.
/// A single attempt (the pre-2026-06-13 behaviour) then dropped the
/// server into permanent degraded mode for a transient startup
/// condition — the only recovery was another restart. Retry for up
/// to `max_wait` (comfortably past observed replay durations) so the
/// connection lands as soon as the engine is ready.
pub async fn connect_pool_with_retry(
    url: &str,
    max_wait: std::time::Duration,
) -> Option<BackendPool> {
    let start = std::time::Instant::now();
    let mut delay = std::time::Duration::from_secs(2);
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        match create_pool(url).await {
            Ok(pool) => {
                if attempt == 1 {
                    tracing::info!("postgres connected");
                } else {
                    tracing::info!(
                        attempt,
                        elapsed_secs = start.elapsed().as_secs(),
                        "postgres connected after retry"
                    );
                }
                return Some(pool);
            }
            Err(e) => {
                if start.elapsed() >= max_wait {
                    tracing::warn!(
                        error = %e,
                        attempts = attempt,
                        elapsed_secs = start.elapsed().as_secs(),
                        "postgres connection failed after retries, running in degraded mode"
                    );
                    return None;
                }
                tracing::warn!(
                    error = %e,
                    attempt,
                    next_retry_secs = delay.as_secs(),
                    "postgres connection attempt failed (engine may be replaying WAL), retrying"
                );
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(std::time::Duration::from_secs(15));
            }
        }
    }
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

#[cfg(all(test, feature = "spg"))]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn connects_first_try_on_valid_catalog() {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("spg://{}/t.spg", dir.path().display());

        let pool = connect_pool_with_retry(&url, Duration::from_secs(5)).await;

        assert!(pool.is_some(), "valid catalog should connect");
    }

    #[tokio::test]
    async fn concedes_to_degraded_after_max_wait() {
        // uncreatable catalog path (/dev/null is a file, nothing can be
        // created beneath it): create_pool errors every attempt, so the
        // loop must give up at max_wait and return None rather than spin
        let start = std::time::Instant::now();
        let pool =
            connect_pool_with_retry("spg:///dev/null/nope/x.spg", Duration::from_secs(1)).await;

        assert!(pool.is_none(), "permanent failure must concede to degraded");
        assert!(
            start.elapsed() >= Duration::from_secs(1),
            "must honour max_wait before conceding"
        );
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "must not overshoot max_wait by much (60s headroom for noisy CI runners)"
        );
    }
}
