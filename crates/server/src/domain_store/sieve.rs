//! Per-account Sieve script storage (RFC 5228).

use super::{DomainStore, Result};

impl DomainStore {
    pub async fn get_sieve_script(&self, address: &str) -> Result<Option<String>> {
        let pool = self.pg()?;
        let row =
            sqlx::query_as::<_, (String,)>("SELECT script FROM sieve_scripts WHERE address = $1")
                .bind(address)
                .fetch_optional(pool)
                .await?;
        Ok(row.map(|(s,)| s))
    }

    pub async fn set_sieve_script(&self, address: &str, script: &str, _now: i64) -> Result<()> {
        let pool = self.pg()?;
        sqlx::query(
            "INSERT INTO sieve_scripts (address, script) VALUES ($1, $2) \
             ON CONFLICT (address) DO UPDATE SET script = EXCLUDED.script, updated_at = now()",
        )
        .bind(address)
        .bind(script)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn delete_sieve_script(&self, address: &str) -> Result<bool> {
        let pool = self.pg()?;
        let res = sqlx::query("DELETE FROM sieve_scripts WHERE address = $1")
            .bind(address)
            .execute(pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }
}
