//! Alias CRUD + recipient resolution (account → alias
//! → catch-all → reject). Includes `resolve_recipient`.

use crate::pg::BackendPool;

use super::{Alias, CachedResolution, DomainStore, ResolvedRecipient, Result};

/// Stable i64 id derived from an alias source address. Matches the
/// `alias_id` helper in `fastcore/src/routes/mail_admin.rs` so
/// consumers who look up by id (webapi remove) can carry an id
/// across backends — network-kevy has no serial PK to lean on.
fn alias_id_of(source: &str) -> i64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut h);
    (h.finish() >> 1) as i64
}

fn domain_of(addr: &str) -> String {
    addr.rsplit_once('@').map(|(_, d)| d).unwrap_or("").into()
}

impl DomainStore {
    pub async fn list_aliases(&self) -> Result<Vec<Alias>> {
        // RFC 20260705 Step 3: when the alias_store seam is wired
        // (network kevy backend), it is authoritative — the PG rows
        // stop being written and reading both would show a stale mix.
        if let Some(store) = self.alias_store.clone() {
            let pairs = tokio::task::spawn_blocking(move || store.list())
                .await
                .map_err(|_| super::StoreError::Unavailable)?
                .map_err(|_| super::StoreError::Unavailable)?;
            let mut out: Vec<Alias> = pairs
                .into_iter()
                .map(|(source, target)| Alias {
                    id: alias_id_of(&source),
                    domain: domain_of(&source),
                    source_address: source,
                    target_address: target,
                    alias_type: "alias".into(),
                    active: true,
                    created_at: 0,
                })
                .collect();
            out.sort_by(|a, b| a.source_address.cmp(&b.source_address));
            return Ok(out);
        }

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
        if let Some(store) = self.alias_store.clone() {
            let source_owned = source.to_string();
            let target_owned = target.to_string();
            tokio::task::spawn_blocking(move || store.upsert(&source_owned, &target_owned))
                .await
                .map_err(|_| super::StoreError::Unavailable)?
                .map_err(|_| super::StoreError::Unavailable)?;
            self.kevy_del(&format!("rcpt:{source}"));
            let _ = domain;
            let _ = alias_type; // trait has no type; monolith only writes plain aliases
            return Ok(alias_id_of(source));
        }

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
        self.kevy_del(&format!("rcpt:{source}"));
        Ok(row.0)
    }

    pub async fn remove_alias(&self, id: i64) -> Result<bool> {
        if let Some(store) = self.alias_store.clone() {
            // network kevy has no PK; find by scanning list and matching
            // the deterministic hash so webapi's id-keyed delete route
            // still works after cutover.
            let list = {
                let s = store.clone();
                tokio::task::spawn_blocking(move || s.list())
                    .await
                    .map_err(|_| super::StoreError::Unavailable)?
                    .map_err(|_| super::StoreError::Unavailable)?
            };
            let Some((source, _)) = list.into_iter().find(|(s, _)| alias_id_of(s) == id) else {
                return Ok(false);
            };
            let src_for_del = source.clone();
            let removed = tokio::task::spawn_blocking(move || store.delete(&src_for_del))
                .await
                .map_err(|_| super::StoreError::Unavailable)?
                .map_err(|_| super::StoreError::Unavailable)?;
            self.kevy_del(&format!("rcpt:{source}"));
            return Ok(removed);
        }

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
            self.kevy_del(&format!("rcpt:{source_addr}"));
        }
        Ok(res.rows_affected() > 0)
    }

    #[cfg(feature = "core-rpc")]
    /// Source-keyed upsert — the backend-neutral alias API both cores
    /// serve for the v2 switchable-core boundary (kevy is source-keyed;
    /// PG gets this so webapi + `mailrs-core-sync` drive one identical
    /// route on either backend). `domain` is derived from the source's
    /// `@` part; `alias_type` defaults to `alias`. Idempotent per source.
    pub async fn upsert_alias_by_source(&self, source: &str, target: &str) -> Result<()> {
        if let Some(store) = self.alias_store.clone() {
            let source_owned = source.to_string();
            let target_owned = target.to_string();
            tokio::task::spawn_blocking(move || store.upsert(&source_owned, &target_owned))
                .await
                .map_err(|_| super::StoreError::Unavailable)?
                .map_err(|_| super::StoreError::Unavailable)?;
            self.kevy_del(&format!("rcpt:{source}"));
            return Ok(());
        }
        let pool = self.pg()?;
        let domain = source.rsplit_once('@').map(|(_, d)| d).unwrap_or("");
        sqlx::query(
            "INSERT INTO aliases (source_address, target_address, domain, alias_type) \
             VALUES ($1, $2, $3, 'alias') \
             ON CONFLICT (source_address) DO UPDATE SET target_address = EXCLUDED.target_address",
        )
        .bind(source)
        .bind(target)
        .bind(domain)
        .execute(pool)
        .await?;
        self.kevy_del(&format!("rcpt:{source}"));
        Ok(())
    }

    #[cfg(feature = "core-rpc")]
    /// Source-keyed delete — companion to [`Self::upsert_alias_by_source`].
    pub async fn remove_alias_by_source(&self, source: &str) -> Result<bool> {
        if let Some(store) = self.alias_store.clone() {
            let source_owned = source.to_string();
            let removed = tokio::task::spawn_blocking(move || store.delete(&source_owned))
                .await
                .map_err(|_| super::StoreError::Unavailable)?
                .map_err(|_| super::StoreError::Unavailable)?;
            self.kevy_del(&format!("rcpt:{source}"));
            return Ok(removed);
        }
        let pool = self.pg()?;
        let res = sqlx::query("DELETE FROM aliases WHERE source_address = $1")
            .bind(source)
            .execute(pool)
            .await?;
        self.kevy_del(&format!("rcpt:{source}"));
        Ok(res.rows_affected() > 0)
    }

    /// resolve a recipient address to local account, forward, or reject
    /// resolution order: exact account → exact alias → catch-all → Reject
    pub async fn resolve_recipient(&self, address: &str) -> ResolvedRecipient {
        // try kevy cache
        let cache_key = format!("rcpt:{address}");
        if let Some(cached) = self.kevy_get::<CachedResolution>(&cache_key) {
            return cached.into();
        }

        let result = self.resolve_recipient_inner(address).await;

        // cache the result
        let cacheable: CachedResolution = (&result).into();
        self.kevy_set(&cache_key, &cacheable, 300);

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

        // 3. exact alias match — RFC 20260705 Step 3: when an
        // AliasStore is wired (network kevy), consult it first so the
        // monolith reads the same alias source of truth as fastcore.
        // The trait is sync; we run it on a blocking pool.
        if let Some(target) = self.resolve_alias_via_store(address).await {
            let targets = vec![(target, "alias".to_string())];
            return self.resolve_targets(pool, &targets).await;
        }
        if self.alias_store.is_none() {
            // Legacy PG path — only run when no alias_store is attached,
            // otherwise the AliasStore is authoritative and its miss is
            // final (matches fastcore's alias-then-reject semantics).
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
        }

        // 4. catch-all (*@domain) — same split as above.
        if let Some((_, domain)) = address.split_once('@') {
            let catchall = format!("*@{domain}");
            if let Some(target) = self.resolve_alias_via_store(&catchall).await {
                let targets = vec![(target, "alias".to_string())];
                return self.resolve_targets(pool, &targets).await;
            }
            if self.alias_store.is_none() {
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
        }

        ResolvedRecipient::Reject
    }

    /// If a backend-agnostic `AliasStore` is attached, resolve `source`
    /// through it on a blocking pool (the trait is sync). Returns
    /// `None` on miss OR on network error — hot-path callers should
    /// treat both as "no alias" and fall through. IO errors are
    /// logged so an oncall can spot a bad network kevy without
    /// silently rejecting mail.
    async fn resolve_alias_via_store(&self, source: &str) -> Option<String> {
        let store = self.alias_store.as_ref()?.clone();
        let source_owned = source.to_string();
        match tokio::task::spawn_blocking(move || store.resolve(&source_owned)).await {
            Ok(Ok(target)) => target,
            Ok(Err(e)) => {
                tracing::warn!(
                    error = %e, source = %source,
                    "alias_store.resolve returned io error — treating as miss"
                );
                None
            }
            Err(e) => {
                tracing::warn!(
                    error = %e, source = %source,
                    "alias_store.resolve blocking task panicked — treating as miss"
                );
                None
            }
        }
    }

    async fn resolve_targets(
        &self,
        pool: &BackendPool,
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
