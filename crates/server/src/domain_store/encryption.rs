//! Per-account encryption-key storage.

use super::{DomainStore, Result};

impl DomainStore {
    /// get an encryption key by address and type; returns (id, public_key, fingerprint)
    pub async fn get_encryption_key(
        &self,
        address: &str,
        key_type: &str,
    ) -> Result<Option<(i64, String, String)>> {
        let pool = self.pg()?;
        let row = sqlx::query_as::<_, (i64, String, String)>(
            "SELECT id, public_key, fingerprint FROM encryption_keys \
             WHERE account_address = $1 AND key_type = $2",
        )
        .bind(address)
        .bind(key_type)
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    /// upsert an encryption key; returns the row id
    pub async fn set_encryption_key(
        &self,
        address: &str,
        key_type: &str,
        public_key: &str,
        fingerprint: &str,
    ) -> Result<i64> {
        let pool = self.pg()?;
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO encryption_keys (account_address, key_type, public_key, fingerprint) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (account_address, key_type) \
             DO UPDATE SET public_key = $3, fingerprint = $4 \
             RETURNING id",
        )
        .bind(address)
        .bind(key_type)
        .bind(public_key)
        .bind(fingerprint)
        .fetch_one(pool)
        .await?;
        Ok(id)
    }

    /// delete an encryption key; returns true if a row was deleted
    pub async fn delete_encryption_key(
        &self,
        address: &str,
        key_type: &str,
    ) -> Result<bool> {
        let pool = self.pg()?;
        let res = sqlx::query(
            "DELETE FROM encryption_keys WHERE account_address = $1 AND key_type = $2",
        )
        .bind(address)
        .bind(key_type)
        .execute(pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    /// list all encryption keys for an account; returns (id, key_type, fingerprint, created_at epoch)
    pub async fn list_encryption_keys(
        &self,
        address: &str,
    ) -> Result<Vec<(i64, String, String, i64)>> {
        let pool = self.pg()?;
        let rows = sqlx::query_as::<_, (i64, String, String, i64)>(
            "SELECT id, key_type, fingerprint, \
             EXTRACT(EPOCH FROM created_at)::bigint \
             FROM encryption_keys WHERE account_address = $1 \
             ORDER BY created_at",
        )
        .bind(address)
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }
}
