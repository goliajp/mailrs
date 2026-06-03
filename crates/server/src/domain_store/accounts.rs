//! Account CRUD handlers + L3 in-process cache pre-load.

use std::time::Instant;

use super::{Account, CACHE_TTL_SECS, CachedAccount, DomainStore, Result};

impl DomainStore {
    /// preload all accounts into process cache for L3 degradation
    pub async fn preload_accounts(&self) {
        let Ok(pool) = self.pg() else { return };
        let rows = sqlx::query_as::<_, (String, String, String, bool, i64, i64, String, String)>(
            "SELECT address, domain, display_name, active, \
             EXTRACT(EPOCH FROM created_at)::bigint, quota_bytes, password_hash, recovery_email \
             FROM accounts",
        )
        .fetch_all(pool)
        .await;

        if let Ok(rows) = rows {
            for (
                address,
                domain,
                display_name,
                active,
                created_at,
                quota_bytes,
                password_hash,
                recovery_email,
            ) in rows
            {
                let account = Account {
                    address: address.clone(),
                    domain,
                    display_name,
                    active,
                    created_at,
                    quota_bytes,
                    recovery_email,
                };
                self.account_cache.insert(
                    address,
                    CachedAccount {
                        account,
                        password_hash,
                        cached_at: Instant::now(),
                    },
                );
            }
        }
    }

    pub async fn list_accounts(&self) -> Result<Vec<Account>> {
        let pool = self.pg()?;
        let rows = sqlx::query_as::<_, (String, String, String, bool, i64, i64, String)>(
            "SELECT address, domain, display_name, active, \
             EXTRACT(EPOCH FROM created_at)::bigint, quota_bytes, recovery_email \
             FROM accounts ORDER BY address",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    address,
                    domain,
                    display_name,
                    active,
                    created_at,
                    quota_bytes,
                    recovery_email,
                )| Account {
                    address,
                    domain,
                    display_name,
                    active,
                    created_at,
                    quota_bytes,
                    recovery_email,
                },
            )
            .collect())
    }

    pub async fn add_account(
        &self,
        address: &str,
        domain: &str,
        display_name: &str,
        password_hash: &str,
        _now: i64,
    ) -> Result<()> {
        let pool = self.pg()?;
        let (active, created_epoch, quota_bytes, recovery_email): (bool, i64, i64, String) =
            sqlx::query_as(
                "INSERT INTO accounts (address, domain, display_name, password_hash) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (address) DO UPDATE SET \
             domain = EXCLUDED.domain, display_name = EXCLUDED.display_name, \
             password_hash = EXCLUDED.password_hash \
             RETURNING active, EXTRACT(EPOCH FROM created_at)::bigint, quota_bytes, recovery_email",
            )
            .bind(address)
            .bind(domain)
            .bind(display_name)
            .bind(password_hash)
            .fetch_one(pool)
            .await?;

        // update caches with actual PG state
        let account = Account {
            address: address.to_string(),
            domain: domain.to_string(),
            display_name: display_name.to_string(),
            active,
            created_at: created_epoch,
            quota_bytes,
            recovery_email,
        };
        self.account_cache.insert(
            address.to_string(),
            CachedAccount {
                account,
                password_hash: password_hash.to_string(),
                cached_at: Instant::now(),
            },
        );
        self.kevy_del(&format!("acct:{address}"));
        self.kevy_del(&format!("rcpt:{address}"));

        Ok(())
    }

    pub async fn update_account_display_name(
        &self,
        address: &str,
        display_name: &str,
    ) -> Result<bool> {
        let pool = self.pg()?;
        let rows = sqlx::query("UPDATE accounts SET display_name = $2 WHERE address = $1")
            .bind(address)
            .bind(display_name)
            .execute(pool)
            .await?
            .rows_affected();
        if rows > 0 {
            // invalidate caches
            self.account_cache.remove(address);
            self.kevy_del(&format!("acct:{address}"));
        }
        Ok(rows > 0)
    }

    pub async fn get_account_with_hash(&self, address: &str) -> Result<Option<(Account, String)>> {
        // try kevy cache
        let cache_key = format!("acct:{address}");
        if let Some(cached) = self.kevy_get::<CachedAccount>(&cache_key) {
            return Ok(Some((cached.account, cached.password_hash)));
        }

        // try PG
        if let Ok(pool) = self.pg() {
            let row = sqlx::query_as::<_, (String, String, String, bool, i64, i64, String, String)>(
                "SELECT address, domain, display_name, active, \
                 EXTRACT(EPOCH FROM created_at)::bigint, quota_bytes, password_hash, recovery_email \
                 FROM accounts WHERE address = $1",
            )
            .bind(address)
            .fetch_optional(pool)
            .await?;

            if let Some((
                addr,
                domain,
                display_name,
                active,
                created_at,
                quota_bytes,
                hash,
                recovery_email,
            )) = row
            {
                let account = Account {
                    address: addr,
                    domain,
                    display_name,
                    active,
                    created_at,
                    quota_bytes,
                    recovery_email,
                };
                let cached = CachedAccount {
                    account: account.clone(),
                    password_hash: hash.clone(),
                    cached_at: Instant::now(),
                };
                // backfill caches
                self.kevy_set(&cache_key, &cached, 300);
                self.account_cache.insert(address.to_string(), cached);
                return Ok(Some((account, hash)));
            }
            return Ok(None);
        }

        // L3 fallback: process cache
        if let Some(entry) = self.account_cache.get(address) {
            if entry.cached_at.elapsed().as_secs() < CACHE_TTL_SECS {
                return Ok(Some((entry.account.clone(), entry.password_hash.clone())));
            }
            drop(entry);
            self.account_cache.remove(address);
        }

        Ok(None)
    }

    pub async fn remove_account(&self, address: &str) -> Result<bool> {
        let pool = self.pg()?;
        let res = sqlx::query("DELETE FROM accounts WHERE address = $1")
            .bind(address)
            .execute(pool)
            .await?;
        self.account_cache.remove(address);
        self.kevy_del(&format!("acct:{address}"));
        self.kevy_del(&format!("rcpt:{address}"));
        Ok(res.rows_affected() > 0)
    }

    pub async fn get_quota(&self, address: &str) -> Result<Option<i64>> {
        if let Ok(Some((account, _))) = self.get_account_with_hash(address).await {
            return Ok(Some(account.quota_bytes));
        }
        Ok(None)
    }

    pub async fn set_quota(&self, address: &str, quota_bytes: i64) -> Result<bool> {
        let pool = self.pg()?;
        let res = sqlx::query("UPDATE accounts SET quota_bytes = $1 WHERE address = $2")
            .bind(quota_bytes)
            .bind(address)
            .execute(pool)
            .await?;
        // invalidate caches
        if let Some(mut entry) = self.account_cache.get_mut(address) {
            entry.account.quota_bytes = quota_bytes;
        }
        self.kevy_del(&format!("acct:{address}"));
        Ok(res.rows_affected() > 0)
    }

    pub async fn update_recovery_email(&self, address: &str, recovery_email: &str) -> Result<bool> {
        let pool = self.pg()?;
        let res = sqlx::query("UPDATE accounts SET recovery_email = $1 WHERE address = $2")
            .bind(recovery_email)
            .bind(address)
            .execute(pool)
            .await?;
        // invalidate caches
        if let Some(mut entry) = self.account_cache.get_mut(address) {
            entry.account.recovery_email = recovery_email.to_string();
        }
        self.kevy_del(&format!("acct:{address}"));
        Ok(res.rows_affected() > 0)
    }
}
