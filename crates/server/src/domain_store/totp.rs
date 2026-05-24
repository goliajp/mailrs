//! TOTP 2FA secret + recovery-code storage handlers.

use super::{DomainStore, Result};

impl DomainStore {
    /// get TOTP secret, enabled status, and recovery codes for an account
    pub async fn get_totp_secret(
        &self,
        address: &str,
    ) -> Result<Option<(String, bool, String)>> {
        let pool = self.pg()?;
        let row = sqlx::query_as::<_, (String, bool, String)>(
            "SELECT secret, enabled, recovery_codes FROM totp_secrets \
             WHERE account_address = $1",
        )
        .bind(address)
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    /// save (upsert) a TOTP secret and recovery codes for an account
    pub async fn save_totp_secret(
        &self,
        address: &str,
        secret: &str,
        recovery_codes: &str,
    ) -> Result<()> {
        let pool = self.pg()?;
        sqlx::query(
            "INSERT INTO totp_secrets (account_address, secret, recovery_codes) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (account_address) \
             DO UPDATE SET secret = $2, recovery_codes = $3, enabled = false",
        )
        .bind(address)
        .bind(secret)
        .bind(recovery_codes)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// enable TOTP for an account (returns true if a row was updated)
    pub async fn enable_totp(&self, address: &str) -> Result<bool> {
        let pool = self.pg()?;
        let result = sqlx::query(
            "UPDATE totp_secrets SET enabled = true WHERE account_address = $1",
        )
        .bind(address)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// disable and delete TOTP for an account (returns true if a row was deleted)
    pub async fn disable_totp(&self, address: &str) -> Result<bool> {
        let pool = self.pg()?;
        let result = sqlx::query(
            "DELETE FROM totp_secrets WHERE account_address = $1",
        )
        .bind(address)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// consume a recovery code (remove it from the list); returns true if code was valid
    pub async fn consume_recovery_code(&self, address: &str, code: &str) -> Result<bool> {
        let pool = self.pg()?;
        let row = sqlx::query_as::<_, (String,)>(
            "SELECT recovery_codes FROM totp_secrets \
             WHERE account_address = $1 AND enabled = true",
        )
        .bind(address)
        .fetch_optional(pool)
        .await?;

        let Some((codes_str,)) = row else {
            return Ok(false);
        };

        let codes: Vec<&str> = codes_str.split(',').collect();
        if !codes.contains(&code) {
            return Ok(false);
        }

        let remaining: Vec<&str> = codes.into_iter().filter(|c| *c != code).collect();
        let new_codes = remaining.join(",");

        sqlx::query(
            "UPDATE totp_secrets SET recovery_codes = $1 WHERE account_address = $2",
        )
        .bind(&new_codes)
        .bind(address)
        .execute(pool)
        .await?;

        Ok(true)
    }
}
