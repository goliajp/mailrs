use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

pub async fn create_pool(url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(20)
        .min_connections(2)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .idle_timeout(std::time::Duration::from_secs(600))
        .max_lifetime(std::time::Duration::from_secs(1800))
        .test_before_acquire(true)
        .connect(url)
        .await
}
