//! Alias CRUD + recipient resolution (account → alias
//! → catch-all → reject). Includes `resolve_recipient`.

use sqlx::PgPool;

use super::{Alias, CachedResolution, DomainStore, ResolvedRecipient, Result};

impl DomainStore {
    pub async fn list_aliases(&self) -> Result<Vec<Alias>> {
        let pool = self.pg()?;
        let rows = sqlx::query_as::<_, (i64, String, String, String, String, bool, i64)>(
            "SELECT id, source_address, target_address, domain, alias_type, active, \
             EXTRACT(EPOCH FROM created_at)::bigint \
             FROM aliases ORDER BY source_address",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(id, source_address, target_address, domain, alias_type, active, created_at)| {
                    Alias {
                        id,
                        source_address,
                        target_address,
                        domain,
                        alias_type,
                        active,
                        created_at,
                    }
                },
            )
            .collect())
    }

    pub async fn add_alias(
        &self,
        source: &str,
        target: &str,
        domain: &str,
        alias_type: &str,
        _now: i64,
    ) -> Result<i64> {
        let pool = self.pg()?;
        let row = sqlx::query_as::<_, (i64,)>(
            "INSERT INTO aliases (source_address, target_address, domain, alias_type) \
             VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(source)
        .bind(target)
        .bind(domain)
        .bind(alias_type)
        .fetch_one(pool)
        .await?;
        // invalidate recipient cache
        self.valkey_del(&format!("rcpt:{source}")).await;
        Ok(row.0)
    }

    pub async fn remove_alias(&self, id: i64) -> Result<bool> {
        let pool = self.pg()?;
        // get source_address before deleting for cache invalidation
        let source =
            sqlx::query_as::<_, (String,)>("SELECT source_address FROM aliases WHERE id = $1")
                .bind(id)
                .fetch_optional(pool)
                .await?;

        let res = sqlx::query("DELETE FROM aliases WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;

        if let Some((source_addr,)) = source {
            self.valkey_del(&format!("rcpt:{source_addr}")).await;
        }
        Ok(res.rows_affected() > 0)
    }

    /// resolve a recipient address to local account, forward, or reject
    /// resolution order: exact account → exact alias → catch-all → Reject
    pub async fn resolve_recipient(&self, address: &str) -> ResolvedRecipient {
        // try valkey cache
        let cache_key = format!("rcpt:{address}");
        if let Some(cached) = self.valkey_get::<CachedResolution>(&cache_key).await {
            return cached.into();
        }

        let result = self.resolve_recipient_inner(address).await;

        // cache the result
        let cacheable: CachedResolution = (&result).into();
        self.valkey_set(&cache_key, &cacheable, 300).await;

        result
    }

    async fn resolve_recipient_inner(&self, address: &str) -> ResolvedRecipient {
        let pool = match self.pg() {
            Ok(p) => p,
            Err(_) => {
                // L3 fallback: check process cache
                if self.account_cache.contains_key(address) {
                    return ResolvedRecipient::Account(address.to_string());
                }
                // when uncertain, accept
                return ResolvedRecipient::Account(address.to_string());
            }
        };

        // 1. exact account match
        let has_account = sqlx::query_as::<_, (bool,)>(
            "SELECT true FROM accounts WHERE address = $1 AND active = true",
        )
        .bind(address)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .is_some();

        if has_account {
            return ResolvedRecipient::Account(address.to_string());
        }

        // 2. email group match
        let group_members: Vec<(String,)> = sqlx::query_as(
            "SELECT m.member_address FROM email_group_members m \
             JOIN email_groups g ON g.id = m.group_id \
             WHERE g.address = $1",
        )
        .bind(address)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        if !group_members.is_empty() {
            let members: Vec<String> = group_members.into_iter().map(|(a,)| a).collect();
            return ResolvedRecipient::Group(members);
        }

        // 3. exact alias match
        let targets: Vec<(String, String)> = sqlx::query_as(
            "SELECT target_address, alias_type FROM aliases \
             WHERE source_address = $1 AND active = true",
        )
        .bind(address)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        if !targets.is_empty() {
            return self.resolve_targets(pool, &targets).await;
        }

        // 3. catch-all (*@domain)
        if let Some((_, domain)) = address.split_once('@') {
            let catchall = format!("*@{domain}");
            let targets: Vec<(String, String)> = sqlx::query_as(
                "SELECT target_address, alias_type FROM aliases \
                 WHERE source_address = $1 AND active = true",
            )
            .bind(&catchall)
            .fetch_all(pool)
            .await
            .unwrap_or_default();

            if !targets.is_empty() {
                return self.resolve_targets(pool, &targets).await;
            }
        }

        ResolvedRecipient::Reject
    }

    async fn resolve_targets(
        &self,
        pool: &PgPool,
        targets: &[(String, String)],
    ) -> ResolvedRecipient {
        let mut local_accounts = Vec::new();
        let mut forwards = Vec::new();

        for (target, alias_type) in targets {
            if alias_type == "forward" {
                forwards.push(target.clone());
            } else {
                let is_local = sqlx::query_as::<_, (bool,)>(
                    "SELECT true FROM accounts WHERE address = $1 AND active = true",
                )
                .bind(target)
                .fetch_optional(pool)
                .await
                .ok()
                .flatten()
                .is_some();

                if is_local {
                    local_accounts.push(target.clone());
                } else {
                    forwards.push(target.clone());
                }
            }
        }

        if !local_accounts.is_empty() && forwards.is_empty() {
            ResolvedRecipient::Account(local_accounts.into_iter().next().unwrap())
        } else if !forwards.is_empty() {
            forwards.extend(local_accounts);
            ResolvedRecipient::Forward(forwards)
        } else {
            ResolvedRecipient::Reject
        }
    }
}
