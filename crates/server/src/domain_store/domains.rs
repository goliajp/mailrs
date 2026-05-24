//! Domain CRUD handlers.

use super::{Domain, DomainStore, Result};

impl DomainStore {
    pub async fn list_domains(&self) -> Result<Vec<Domain>> {
        let pool = self.pg()?;
        let rows = sqlx::query_as::<_, (String, i64)>(
            "SELECT name, EXTRACT(EPOCH FROM created_at)::bigint FROM domains ORDER BY name",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(name, created_at)| Domain { name, created_at })
            .collect())
    }

    pub async fn add_domain(&self, name: &str, _now: i64) -> Result<()> {
        let pool = self.pg()?;
        sqlx::query("INSERT INTO domains (name) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(name)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn remove_domain(&self, name: &str) -> Result<bool> {
        let pool = self.pg()?;
        // collect affected accounts before cascade delete
        let addresses: Vec<(String,)> =
            sqlx::query_as("SELECT address FROM accounts WHERE domain = $1")
                .bind(name)
                .fetch_all(pool)
                .await?;

        // cascade deletes accounts, aliases via FK
        let res = sqlx::query("DELETE FROM domains WHERE name = $1")
            .bind(name)
            .execute(pool)
            .await?;

        // invalidate process cache
        self.account_cache.retain(|_, v| v.account.domain != name);

        // invalidate Valkey cache for all affected accounts
        for (addr,) in &addresses {
            self.valkey_del(&format!("acct:{addr}")).await;
            self.valkey_del(&format!("rcpt:{addr}")).await;
        }

        Ok(res.rows_affected() > 0)
    }
}
